//! Turning scanned evidence into a circuit proposal — or into a question.
//!
//! [`crate::cppscan`] answers *what does the text say*; this module answers
//! *what does that imply about the hardware*, which is a different and more
//! dangerous question. A wrong answer here is a wiring claim the user was
//! invited to accept, on hardware that can be destroyed by acting on it.
//!
//! So every ambiguity resolves toward silence plus a finding. A missed
//! component costs the user one entry in a form that already exists; a
//! fabricated one costs trust in every other row of the proposal.
//!
//! Three rules give that principle teeth:
//!
//! 1. **A GPIO is only ever claimed at [`Evidence::Stated`]** — the sketch's
//!    own source states the number, or the core's variant header does. A
//!    recipe may imply that a part exists and which rail it runs from; it may
//!    never imply which pin it landed on.
//! 2. **Nothing safety-related is ever written.** No `verified`, no series
//!    resistor, no pull-ups, no driver or flyback flags. A proposal therefore
//!    arrives *invalid on purpose*, carrying the exact diagnostics acceptance
//!    will produce, as a checklist of what only a human can confirm.
//! 3. **Merges are additive.** A guess never edits or deletes what somebody
//!    wrote by hand; disagreement is reported, not applied.

use std::collections::{BTreeMap, BTreeSet};

use crate::circuit::{
    BoardProfile, CircuitManifest, Component, Connection, Diagnostic, Endpoint, PartPin, Severity,
};
use crate::cppscan::{fold, Arg, Definition, Scan, SourceSite, Unfoldable};
use crate::partsdb::{self, Bus, PartRecipe};
use crate::variantpins::{CoreSymbol, VariantPins};

/// How strong the evidence behind one proposed item is.
///
/// Two tiers, mirroring [`crate::fleet::BoardIdKind`]: this repo already
/// models evidence quality as a small enum with a doc explaining why one tier
/// is stronger, and already refuses to print a number it has not measured.
///
/// A third tier — *verified* — is structurally absent. Only a human with the
/// module in hand and its datasheet open produces that, by ticking
/// `verified: true`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Evidence {
    /// The project's own source, or the core's variant header for this board,
    /// states the number outright.
    Stated,
    /// A cited recipe implies it — the part exists, and it runs from this
    /// rail. Never attached to a GPIO assignment.
    Implied,
}

impl Evidence {
    /// The wording used in the UI, taken from the vocabulary already in
    /// `.agents/skills/esp32-circuit-designer/references/safety-and-review.md`.
    pub fn label(self) -> &'static str {
        match self {
            Evidence::Stated => "stated in the sketch",
            Evidence::Implied => "likely — verify on the module",
        }
    }
}

/// Where the board being guessed against came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoardSource {
    /// An existing `circuit.yaml` already names it — the user's stated truth.
    Manifest,
    /// Derived from the active build profile.
    Profile,
    /// The user picked it explicitly for this guess.
    Selected,
}

/// The board a proposal is written against, and why that one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuessBoard {
    pub catalog_id: String,
    pub fqbn: String,
    pub source: BoardSource,
    /// The variant header alias resolution came from, or empty when no core is
    /// installed.
    pub variant_source: String,
}

/// One proposed wire. Cannot exist without its component.
#[derive(Debug, Clone, PartialEq)]
pub struct ProposedConnection {
    pub item_id: String,
    /// The component `item_id` this wire depends on.
    pub requires: String,
    pub connection: Connection,
    pub evidence: Evidence,
    /// One sentence naming what in the source implies this wire.
    pub why: String,
    pub sites: Vec<SourceSite>,
}

/// One proposed component and everything wired to it. The unit of acceptance.
#[derive(Debug, Clone, PartialEq)]
pub struct ProposedComponent {
    pub item_id: String,
    pub component: Component,
    pub evidence: Evidence,
    pub why: String,
    pub sites: Vec<SourceSite>,
    /// The datasheet the pin names and rail were read from.
    pub source_url: String,
    /// What the human must check on the actual module.
    pub verify: String,
    pub connections: Vec<ProposedConnection>,
    /// Diagnostics accepting this item will produce, filled by the merge step.
    pub todo: Vec<Diagnostic>,
    /// A component with this id already exists in the project's manifest.
    pub conflicts_with_existing: bool,
}

/// A complete proposal. Nothing here exists on disk.
#[derive(Debug, Clone, PartialEq)]
pub struct CircuitGuess {
    pub board: GuessBoard,
    /// An empty list is a legitimate answer, and the common one.
    pub components: Vec<ProposedComponent>,
    /// Seen but not claimed. Always [`Severity::Info`] under a `guess.` code.
    pub findings: Vec<Diagnostic>,
}

fn info(code: &str, subject: Option<&str>, message: String) -> Diagnostic {
    Diagnostic {
        severity: Severity::Info,
        code: code.to_string(),
        message,
        subject: subject.map(|s| s.to_string()),
    }
}

/// One resolved bus pin, with the evidence behind it.
struct BusPin {
    gpio: u8,
    why: String,
    sites: Vec<SourceSite>,
}

/// A pin resolution attempt.
enum Pin {
    At {
        gpio: u8,
        evidence: Evidence,
        why: String,
        sites: Vec<SourceSite>,
    },
    Refused(Diagnostic),
}

/// Resolve an argument to a GPIO on this board, or explain why not.
///
/// The ladder, in order:
/// 1. A literal in the sketch.
/// 2. A name the sketch defines inside `#ifndef NAME` **while the core also
///    defines NAME** — the guard does not fire, so the core's value governs,
///    and when the core's value is not a literal nothing is claimed.
/// 3. A name the sketch defines outright (or defines under a guard the core
///    does not pre-empt).
/// 4. A name only the core's variant header binds.
/// 5. Otherwise a finding.
fn resolve_pin(
    arg: &Arg,
    defs: &[Definition],
    pins: &VariantPins,
    board: &BoardProfile,
    site: &SourceSite,
    subject: &str,
) -> Pin {
    let (value, evidence, why, mut sites) = match arg {
        Arg::Int(v) => (
            *v,
            Evidence::Stated,
            format!("the sketch passes {v} directly"),
            vec![site.clone()],
        ),
        Arg::Other(text) => {
            return Pin::Refused(info(
                "guess.unresolved_pin",
                Some(subject),
                format!(
                    "{} at {}:{} passes `{}`, which is not a plain pin number. \
                     Set this component's pin by hand.",
                    subject, site.rel_path, site.line, text
                ),
            ))
        }
        Arg::Ident(name) => {
            let core = pins.lookup(name);
            match fold(defs, name) {
                Err(Unfoldable::Ambiguous(at)) => {
                    let places = at
                        .iter()
                        .map(|s| format!("{}:{}", s.rel_path, s.line))
                        .collect::<Vec<_>>()
                        .join(" and ");
                    return Pin::Refused(info(
                        "guess.ambiguous_symbol",
                        Some(subject),
                        format!(
                            "`{name}` is defined with different values at {places}. \
                             Picking one would be a guess."
                        ),
                    ));
                }
                // The guard does not fire: the core already defines this name.
                Ok(def) if def.guarded && core.is_defined() => match core {
                    CoreSymbol::Defined(v) => (
                        v,
                        Evidence::Stated,
                        format!(
                            "the core defines `{name}` for this board, so the sketch's \
                             `#ifndef` at {}:{} does not take effect; the core's value is {v}",
                            def.site.rel_path, def.site.line
                        ),
                        vec![site.clone(), def.site.clone()],
                    ),
                    _ => {
                        return Pin::Refused(info(
                            "guess.core_defined_symbol",
                            Some(subject),
                            format!(
                                "`{name}` is defined by this board's core ({}), not as a plain \
                                 pin number — so the sketch's `#ifndef` at {}:{} never takes \
                                 effect and its value is not the pin in use. On an ESP32-S3 this \
                                 is the addressable RGB LED rather than a GPIO. Set this pin by \
                                 hand.",
                                pins.source, def.site.rel_path, def.site.line
                            ),
                        ))
                    }
                },
                Ok(def) => (
                    def.value,
                    Evidence::Stated,
                    format!(
                        "the sketch defines `{name}` as {} at {}:{}",
                        def.value, def.site.rel_path, def.site.line
                    ),
                    vec![site.clone(), def.site.clone()],
                ),
                Err(Unfoldable::Unknown) => match core {
                    CoreSymbol::Defined(v) => (
                        v,
                        Evidence::Stated,
                        format!(
                            "this board's core defines `{name}` as {v} ({})",
                            pins.source
                        ),
                        vec![site.clone()],
                    ),
                    CoreSymbol::DefinedNonLiteral => {
                        return Pin::Refused(info(
                            "guess.core_defined_symbol",
                            Some(subject),
                            format!(
                                "`{name}` is defined by this board's core ({}) as an expression \
                                 rather than a pin number, so it cannot be resolved here.",
                                pins.source
                            ),
                        ))
                    }
                    CoreSymbol::Absent => {
                        return Pin::Refused(info(
                            "guess.unresolved_pin",
                            Some(subject),
                            format!(
                                "`{name}` at {}:{} is never given a plain number in this project, \
                                 and this board's core does not define it either.",
                                site.rel_path, site.line
                            ),
                        ))
                    }
                },
            }
        }
    };

    if value < 0 {
        return Pin::Refused(info(
            "guess.board_default_pin",
            Some(subject),
            format!(
                "the sketch asks for this board's default pin by passing {value}, rather than \
                 naming one. Which pin that is depends on the core, so it is not claimed here."
            ),
        ));
    }
    let Ok(gpio) = u8::try_from(value) else {
        return Pin::Refused(info(
            "guess.pin_not_on_board",
            Some(subject),
            format!("{value} is not a GPIO number on {}", board.label),
        ));
    };
    let id = format!("GPIO{gpio}");
    let Some(pin) = board.pins.iter().find(|p| p.id == id) else {
        return Pin::Refused(info(
            "guess.pin_not_on_board",
            Some(subject),
            format!("{id} does not exist on {}", board.label),
        ));
    };
    if pin.reserved {
        return Pin::Refused(info(
            "guess.pin_reserved",
            Some(subject),
            format!(
                "{id} is reserved on {} and should not be wired to a component.",
                board.label
            ),
        ));
    }
    sites.dedup_by(|a, b| a == b);
    Pin::At {
        gpio,
        evidence,
        why,
        sites,
    }
}

/// The I²C pins in force, from `Wire.begin(sda, scl)` when the sketch states
/// them, otherwise from the core's `SDA`/`SCL`.
fn i2c_pins(
    scan: &Scan,
    pins: &VariantPins,
    board: &BoardProfile,
    subject: &str,
) -> Result<(BusPin, BusPin), Vec<Diagnostic>> {
    let mut refusals = Vec::new();
    if let Some(call) = scan
        .calls
        .iter()
        .find(|c| c.name == "Wire.begin" && c.args.len() == 2)
    {
        let sda = resolve_pin(
            &call.args[0],
            &scan.definitions,
            pins,
            board,
            &call.site,
            subject,
        );
        let scl = resolve_pin(
            &call.args[1],
            &scan.definitions,
            pins,
            board,
            &call.site,
            subject,
        );
        if let (
            Pin::At {
                gpio: a,
                why: wa,
                sites: sa,
                ..
            },
            Pin::At {
                gpio: b,
                why: wb,
                sites: sb,
                ..
            },
        ) = (&sda, &scl)
        {
            return Ok((
                BusPin {
                    gpio: *a,
                    why: wa.clone(),
                    sites: sa.clone(),
                },
                BusPin {
                    gpio: *b,
                    why: wb.clone(),
                    sites: sb.clone(),
                },
            ));
        }
        for outcome in [sda, scl] {
            if let Pin::Refused(d) = outcome {
                refusals.push(d);
            }
        }
    }
    // No usable explicit call — fall back to what the core says the defaults are.
    match (pins.lookup("SDA"), pins.lookup("SCL")) {
        (CoreSymbol::Defined(a), CoreSymbol::Defined(b)) => {
            let (Ok(a), Ok(b)) = (u8::try_from(a), u8::try_from(b)) else {
                return Err(refusals);
            };
            let why = format!(
                "this board's core puts I2C on GPIO{a}/GPIO{b} ({})",
                pins.source
            );
            Ok((
                BusPin {
                    gpio: a,
                    why: why.clone(),
                    sites: Vec::new(),
                },
                BusPin {
                    gpio: b,
                    why,
                    sites: Vec::new(),
                },
            ))
        }
        _ => {
            refusals.push(info(
                "guess.unresolved_pin",
                Some(subject),
                format!(
                    "the sketch does not state I2C pins and this board's core does not \
                     supply defaults{}, so no bus wires are proposed.",
                    if pins.is_empty() {
                        " (no installed core was found)"
                    } else {
                        ""
                    }
                ),
            ));
            Err(refusals)
        }
    }
}

fn part_pin(recipe: &PartRecipe, name: Option<&str>) -> Option<String> {
    name.map(|n| n.to_string())
        .filter(|n| recipe.pins.contains(&n.as_str()))
}

/// Build a proposal from scanned evidence.
///
/// Pure: no filesystem, no clock, no ordering surprises. `base` is the
/// project's current manifest when it has one, used only to *avoid* colliding
/// with what is already there — never to modify it.
pub fn guess(
    scan: &Scan,
    board: &BoardProfile,
    board_source: BoardSource,
    pins: &VariantPins,
    base: Option<&CircuitManifest>,
) -> CircuitGuess {
    let mut components: Vec<ProposedComponent> = Vec::new();
    let mut findings: Vec<Diagnostic> = Vec::new();

    let included: BTreeSet<&str> = scan.includes.iter().map(|i| i.header.as_str()).collect();
    let taken_ids: BTreeSet<String> = base
        .map(|m| m.components.iter().map(|c| c.id.clone()).collect())
        .unwrap_or_default();
    // GPIOs the existing manifest already wires. Proposing one again would
    // author a `board.pin_conflict` the user never asked for.
    let taken_gpios: BTreeSet<String> = base
        .map(|m| {
            m.connections
                .iter()
                .flat_map(|c| [c.from.pin.clone(), c.to.pin.clone()])
                .filter(|p| p.starts_with("GPIO"))
                .collect()
        })
        .unwrap_or_default();

    let mut used_stems: BTreeMap<String, usize> = BTreeMap::new();
    let mut instantiations = scan.instantiations.clone();
    instantiations.sort_by(|a, b| {
        (a.site.rel_path.as_str(), a.site.line, a.class_name.as_str()).cmp(&(
            b.site.rel_path.as_str(),
            b.site.line,
            b.class_name.as_str(),
        ))
    });

    for inst in &instantiations {
        let Some(recipe) = partsdb::by_class(&inst.class_name) else {
            continue;
        };
        // The class alone is not enough: requiring the header too keeps a
        // same-named local class from impersonating a part.
        if !included.contains(partsdb::include_of(recipe)) {
            continue;
        }

        let seen = used_stems.entry(recipe.id.to_string()).or_insert(0);
        *seen += 1;
        let mut item_id = if *seen == 1 {
            recipe.id.to_string()
        } else {
            format!("{}_{}", recipe.id, seen)
        };
        let conflicts = taken_ids.contains(&item_id);
        if conflicts {
            let mut n = *seen;
            while taken_ids.contains(&item_id) {
                n += 1;
                item_id = format!("{}_{}", recipe.id, n);
            }
            findings.push(info(
                "guess.conflicts_with_existing",
                Some(&item_id),
                format!(
                    "the project already declares a component called `{}`; this one is proposed \
                     as `{item_id}` rather than changing what you wrote.",
                    recipe.id
                ),
            ));
        }

        let component = Component {
            id: item_id.clone(),
            label: recipe.label.to_string(),
            kind: recipe.kind.to_string(),
            part_number: None,
            voltage_v: None,
            current_ma: recipe.current_ma,
            // Never true. Only a human confirms a part against its datasheet.
            verified: false,
            pins: recipe
                .pins
                .iter()
                .map(|p| PartPin {
                    id: (*p).to_string(),
                    label: String::new(),
                })
                .collect(),
            // Never populated: every property the validator cares about is
            // safety evidence.
            properties: BTreeMap::new(),
        };

        let mut connections: Vec<ProposedConnection> = Vec::new();
        let mut wire = |suffix: &str,
                        board_pin: String,
                        part: Option<String>,
                        role: &str,
                        evidence: Evidence,
                        why: String,
                        sites: Vec<SourceSite>,
                        findings: &mut Vec<Diagnostic>| {
            let Some(part) = part else { return };
            if board_pin.starts_with("GPIO") && taken_gpios.contains(&board_pin) {
                findings.push(info(
                    "guess.pin_taken",
                    Some(&item_id),
                    format!(
                        "{board_pin} is already wired in this project's circuit, so no wire is \
                         proposed for {}'s {part}.",
                        recipe.label
                    ),
                ));
                return;
            }
            connections.push(ProposedConnection {
                item_id: format!("{item_id}_{suffix}"),
                requires: item_id.clone(),
                connection: Connection {
                    id: format!("{item_id}_{suffix}"),
                    from: Endpoint {
                        target: "board".into(),
                        pin: board_pin,
                    },
                    to: Endpoint {
                        target: item_id.clone(),
                        pin: part,
                    },
                    role: role.to_string(),
                    // Deliberately unset: this module asserts no voltages.
                    voltage_v: None,
                    // Never proposed — see the module doc.
                    firmware_symbol: None,
                    notes: None,
                },
                evidence,
                why,
                sites,
            });
        };

        let mut evidence = Evidence::Implied;
        let mut why = format!(
            "the sketch includes {} and declares `{} {}`",
            partsdb::include_of(recipe),
            inst.class_name,
            inst.var
        );
        let sites = vec![inst.site.clone()];

        match partsdb::bus_of(recipe) {
            Bus::I2c => match i2c_pins(scan, pins, board, &item_id) {
                Ok((sda, scl)) => {
                    wire(
                        "sda",
                        format!("GPIO{}", sda.gpio),
                        part_pin(recipe, recipe.roles.sda),
                        "i2c_sda",
                        Evidence::Stated,
                        sda.why,
                        sda.sites,
                        &mut findings,
                    );
                    wire(
                        "scl",
                        format!("GPIO{}", scl.gpio),
                        part_pin(recipe, recipe.roles.scl),
                        "i2c_scl",
                        Evidence::Stated,
                        scl.why,
                        scl.sites,
                        &mut findings,
                    );
                }
                Err(refusals) => findings.extend(refusals),
            },
            Bus::Digital | Bus::OneWire | Bus::Analog => {
                match partsdb::pin_arg_of(recipe).and_then(|i| inst.args.get(i)) {
                    Some(arg) => {
                        match resolve_pin(arg, &scan.definitions, pins, board, &inst.site, &item_id)
                        {
                            Pin::At {
                                gpio,
                                evidence: e,
                                why: w,
                                sites: s,
                            } => {
                                evidence = e;
                                why = w.clone();
                                wire(
                                    "signal",
                                    format!("GPIO{gpio}"),
                                    part_pin(recipe, recipe.roles.signal),
                                    "signal",
                                    e,
                                    w,
                                    s,
                                    &mut findings,
                                );
                            }
                            Pin::Refused(d) => findings.push(d),
                        }
                    }
                    None => findings.push(info(
                        "guess.unresolved_pin",
                        Some(&item_id),
                        format!(
                            "`{} {}` at {}:{} does not pass a pin where one was expected.",
                            inst.class_name, inst.var, inst.site.rel_path, inst.site.line
                        ),
                    )),
                }
            }
            Bus::Spi => {}
        }

        // Power and ground travel together: a component on a rail with no
        // ground is `power.missing_ground`, an error we would be authoring.
        if let Some(rail) = recipe.rail {
            let why_rail = format!(
                "{} is a {} part according to its datasheet",
                recipe.label,
                match rail {
                    partsdb::Rail::V3v3 => "3.3 V",
                    partsdb::Rail::V5v => "5 V",
                }
            );
            wire(
                "power",
                rail.pin().to_string(),
                part_pin(recipe, recipe.roles.power),
                "power",
                Evidence::Implied,
                why_rail.clone(),
                Vec::new(),
                &mut findings,
            );
            wire(
                "ground",
                "GND".to_string(),
                part_pin(recipe, recipe.roles.ground),
                "ground",
                Evidence::Implied,
                why_rail,
                Vec::new(),
                &mut findings,
            );
        }

        components.push(ProposedComponent {
            item_id,
            component,
            evidence,
            why,
            sites,
            source_url: recipe.source_url.to_string(),
            verify: recipe.verify.to_string(),
            connections,
            todo: Vec::new(),
            conflicts_with_existing: conflicts,
        });
    }

    CircuitGuess {
        board: GuessBoard {
            catalog_id: board.id.clone(),
            fqbn: board.fqbn.clone(),
            source: board_source,
            variant_source: pins.source.clone(),
        },
        components,
        findings,
    }
}

/// Turn a proposal plus a selection into a manifest.
///
/// **Additive only.** Everything already in `base` survives untouched: this
/// function appends, and never edits or removes a component, a connection, or
/// a board choice. A static analyser does not get to overwrite what a person
/// stated about their own hardware.
///
/// A connection whose component was not accepted is an **error**, not a silent
/// drop. Dropping it quietly would hand back a manifest the user never
/// inspected; the UI cascades its checkboxes so this is a backstop rather than
/// a path anyone walks.
pub fn merge(
    guess: &CircuitGuess,
    base: &CircuitManifest,
    accept: &[String],
) -> crate::Result<CircuitManifest> {
    let wanted: BTreeSet<&str> = accept.iter().map(String::as_str).collect();
    let mut out = base.clone();
    let taken: BTreeSet<String> = base.components.iter().map(|c| c.id.clone()).collect();

    let accepted_components: BTreeSet<&str> = guess
        .components
        .iter()
        .filter(|c| wanted.contains(c.item_id.as_str()))
        .map(|c| c.item_id.as_str())
        .collect();

    // Refuse an orphan before writing anything.
    for component in &guess.components {
        for wire in &component.connections {
            if wanted.contains(wire.item_id.as_str())
                && !accepted_components.contains(wire.requires.as_str())
            {
                return Err(crate::Error::Other(format!(
                    "cannot accept the wire `{}` without the component `{}` it attaches to",
                    wire.item_id, wire.requires
                )));
            }
        }
    }

    for component in &guess.components {
        if !accepted_components.contains(component.item_id.as_str()) {
            continue;
        }
        if taken.contains(&component.item_id) {
            return Err(crate::Error::Other(format!(
                "the project already declares a component called `{}`",
                component.item_id
            )));
        }
        out.components.push(component.component.clone());
        for wire in &component.connections {
            if wanted.contains(wire.item_id.as_str()) {
                out.connections.push(wire.connection.clone());
            }
        }
    }
    Ok(out)
}

/// Every id in a proposal — the "accept everything" selection.
pub fn all_item_ids(guess: &CircuitGuess) -> Vec<String> {
    let mut out = Vec::new();
    for component in &guess.components {
        out.push(component.item_id.clone());
        for wire in &component.connections {
            out.push(wire.item_id.clone());
        }
    }
    out
}

/// Fill each item's `todo` with the diagnostics accepting *everything* would
/// produce, bucketed by the item they name.
///
/// Derived from [`crate::circuit::validate_manifest_for`] rather than
/// hand-maintained, so the checklist a user reads cannot drift from the rules
/// that will actually run. These are the things only a human can settle —
/// `component.unverified` on every part, plus the safety evidence its kind
/// demands — so a proposal legitimately arrives carrying errors.
pub fn annotate_todos(
    guess: &mut CircuitGuess,
    project: &std::path::Path,
    base: &CircuitManifest,
    selected_fqbn: Option<&str>,
) {
    let all = all_item_ids(guess);
    let Ok(merged) = merge(guess, base, &all) else {
        return;
    };
    let diagnostics = crate::circuit::validate_manifest_for(project, &merged, selected_fqbn);
    for component in &mut guess.components {
        component.todo = diagnostics
            .iter()
            .filter(|d| d.subject.as_deref() == Some(component.item_id.as_str()))
            .cloned()
            .collect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::circuit;
    use crate::cppscan;
    use crate::variantpins::parse_variant_header;

    fn board(id: &str) -> BoardProfile {
        circuit::catalog()
            .boards
            .into_iter()
            .find(|b| b.id == id)
            .expect("catalog board")
    }

    /// The S3's real alias set, trimmed to what these tests touch.
    fn s3_pins() -> VariantPins {
        parse_variant_header(
            "variants/esp32s3/pins_arduino.h",
            "static const uint8_t LED_BUILTIN = SOC_GPIO_PIN_COUNT + PIN_RGB_LED;\n\
             #define LED_BUILTIN LED_BUILTIN\n\
             static const uint8_t SDA = 8;\n\
             static const uint8_t SCL = 9;\n\
             static const uint8_t A0 = 1;\n",
        )
    }

    fn classic_pins() -> VariantPins {
        parse_variant_header(
            "variants/esp32/pins_arduino.h",
            "static const uint8_t SDA = 21;\n\
             static const uint8_t SCL = 22;\n\
             static const uint8_t A0 = 36;\n",
        )
    }

    fn guess_src(src: &str, board_id: &str, pins: &VariantPins) -> CircuitGuess {
        let scan = cppscan::scan_file("sketch.ino", src);
        guess(&scan, &board(board_id), BoardSource::Profile, pins, None)
    }

    fn codes(g: &CircuitGuess) -> Vec<&str> {
        g.findings.iter().map(|d| d.code.as_str()).collect()
    }

    #[test]
    fn a_sketch_with_no_recognised_library_proposes_nothing() {
        let g = guess_src(
            "void setup() { pinMode(4, OUTPUT); }\nvoid loop() {}\n",
            "esp32-s3-devkitc-1",
            &s3_pins(),
        );
        assert!(g.components.is_empty(), "{:?}", g.components);
        assert!(g.findings.is_empty(), "{:?}", g.findings);
    }

    #[test]
    fn a_dht_with_a_stated_pin_becomes_one_component_on_that_gpio() {
        let g = guess_src(
            "#include <DHT.h>\n#define DHTPIN 4\nDHT dht(DHTPIN, DHT22);\n",
            "esp32-s3-devkitc-1",
            &s3_pins(),
        );
        assert_eq!(g.components.len(), 1);
        let c = &g.components[0];
        assert_eq!(c.item_id, "dht");
        assert_eq!(c.evidence, Evidence::Stated);
        let signal = c
            .connections
            .iter()
            .find(|w| w.connection.role == "signal")
            .expect("a signal wire");
        assert_eq!(signal.connection.from.pin, "GPIO4");
        assert_eq!(signal.connection.to.pin, "DATA");
        // Power and ground travel together or not at all.
        assert!(c.connections.iter().any(|w| w.connection.role == "power"));
        assert!(c.connections.iter().any(|w| w.connection.role == "ground"));
    }

    #[test]
    fn nothing_proposed_is_ever_verified_or_carries_safety_evidence() {
        // The single most important assertion in this module.
        let g = guess_src(
            "#include <DHT.h>\n#include <Adafruit_NeoPixel.h>\n\
             DHT dht(4, DHT22);\nAdafruit_NeoPixel strip(60, 5, NEO_GRB);\n",
            "esp32-s3-devkitc-1",
            &s3_pins(),
        );
        assert_eq!(g.components.len(), 2);
        for c in &g.components {
            assert!(!c.component.verified, "{} was proposed verified", c.item_id);
            assert!(
                c.component.properties.is_empty(),
                "{} carries properties {:?}",
                c.item_id,
                c.component.properties
            );
            for w in &c.connections {
                assert!(
                    w.connection.firmware_symbol.is_none(),
                    "{} proposed a firmware symbol",
                    w.item_id
                );
            }
        }
    }

    #[test]
    fn a_gpio_is_never_claimed_on_implied_evidence() {
        // The rule that keeps a recipe from inventing a pin: only power and
        // ground may be Implied, and neither touches a GPIO.
        let g = guess_src(
            "#include <DHT.h>\nDHT dht(4, DHT22);\n",
            "esp32-s3-devkitc-1",
            &s3_pins(),
        );
        for w in &g.components[0].connections {
            if w.connection.from.pin.starts_with("GPIO") {
                assert_eq!(
                    w.evidence,
                    Evidence::Stated,
                    "{} claims a GPIO on implied evidence",
                    w.item_id
                );
            }
        }
    }

    #[test]
    fn the_s3_refuses_to_resolve_a_core_defined_led_builtin() {
        // The trap this whole feature is shaped around. blink's guarded
        // `#define LED_BUILTIN 2` never fires on an S3, so 2 is not the pin.
        let src = "#include <Adafruit_NeoPixel.h>\n\
                   #ifndef LED_BUILTIN\n#define LED_BUILTIN 2\n#endif\n\
                   Adafruit_NeoPixel strip(1, LED_BUILTIN, NEO_GRB);\n";
        let g = guess_src(src, "esp32-s3-devkitc-1", &s3_pins());
        assert_eq!(g.components.len(), 1);
        assert!(
            !g.components[0]
                .connections
                .iter()
                .any(|w| w.connection.role == "signal"),
            "claimed a data pin on the S3: {:?}",
            g.components[0].connections
        );
        assert!(
            codes(&g).contains(&"guess.core_defined_symbol"),
            "{:?}",
            g.findings
        );
    }

    #[test]
    fn the_same_sketch_resolves_that_pin_on_a_board_whose_core_leaves_it_undefined() {
        // The other half of the trap: where the core does *not* define
        // LED_BUILTIN, the guard fires and the sketch's 2 really is the pin.
        let src = "#include <Adafruit_NeoPixel.h>\n\
                   #ifndef LED_BUILTIN\n#define LED_BUILTIN 2\n#endif\n\
                   Adafruit_NeoPixel strip(1, LED_BUILTIN, NEO_GRB);\n";
        let g = guess_src(src, "esp32-devkitc-v4", &classic_pins());
        let signal = g.components[0]
            .connections
            .iter()
            .find(|w| w.connection.role == "signal")
            .expect("a signal wire on the classic esp32");
        assert_eq!(signal.connection.from.pin, "GPIO2");
        assert_eq!(signal.evidence, Evidence::Stated);
    }

    #[test]
    fn i2c_pins_come_from_the_core_when_the_sketch_does_not_state_them() {
        let src = "#include <Adafruit_BME280.h>\nAdafruit_BME280 bme;\n";
        let s3 = guess_src(src, "esp32-s3-devkitc-1", &s3_pins());
        let sda = s3.components[0]
            .connections
            .iter()
            .find(|w| w.connection.role == "i2c_sda")
            .expect("sda");
        assert_eq!(sda.connection.from.pin, "GPIO8");
        assert_eq!(sda.connection.to.pin, "SDI");

        // The same sketch, a different board, a different answer — which is
        // why the guess board is selectable.
        let classic = guess_src(src, "esp32-devkitc-v4", &classic_pins());
        let sda = classic.components[0]
            .connections
            .iter()
            .find(|w| w.connection.role == "i2c_sda")
            .expect("sda");
        assert_eq!(sda.connection.from.pin, "GPIO21");
    }

    #[test]
    fn an_explicit_wire_begin_overrides_the_core_defaults() {
        let g = guess_src(
            "#include <Adafruit_BME280.h>\nAdafruit_BME280 bme;\n\
             void setup() { Wire.begin(6, 7); }\n",
            "esp32-s3-devkitc-1",
            &s3_pins(),
        );
        let sda = g.components[0]
            .connections
            .iter()
            .find(|w| w.connection.role == "i2c_sda")
            .unwrap();
        assert_eq!(sda.connection.from.pin, "GPIO6");
    }

    #[test]
    fn a_negative_pin_is_reported_as_a_board_default_not_as_a_gpio() {
        // i2c_scan.ino.tmpl's sentinel: `const int PIN_SDA = -1;`.
        let g = guess_src(
            "#include <Adafruit_BME280.h>\nAdafruit_BME280 bme;\n\
             const int PIN_SDA = -1;\nconst int PIN_SCL = -1;\n\
             void setup() { Wire.begin(PIN_SDA, PIN_SCL); }\n",
            "esp32-s3-devkitc-1",
            &VariantPins::default(),
        );
        assert!(
            !g.components[0]
                .connections
                .iter()
                .any(|w| w.connection.role.starts_with("i2c")),
            "claimed a bus pin from a sentinel"
        );
        assert!(
            codes(&g).contains(&"guess.board_default_pin"),
            "{:?}",
            g.findings
        );
    }

    #[test]
    fn a_component_in_a_comment_or_if_zero_is_not_proposed() {
        for src in [
            "// #include <DHT.h>\n// DHT dht(4, DHT22);\n",
            "#if 0\n#include <DHT.h>\nDHT dht(4, DHT22);\n#endif\n",
        ] {
            let g = guess_src(src, "esp32-s3-devkitc-1", &s3_pins());
            assert!(g.components.is_empty(), "{src:?} -> {:?}", g.components);
        }
    }

    #[test]
    fn a_class_without_its_header_is_not_a_part() {
        // A local class that happens to share a name must not impersonate one.
        let g = guess_src("DHT dht(4, DHT22);\n", "esp32-s3-devkitc-1", &s3_pins());
        assert!(g.components.is_empty(), "{:?}", g.components);
    }

    #[test]
    fn an_ambiguous_symbol_names_both_sites_and_claims_nothing() {
        let g = guess_src(
            "#include <DHT.h>\n#define DHTPIN 4\n#define DHTPIN 5\nDHT dht(DHTPIN, DHT22);\n",
            "esp32-s3-devkitc-1",
            &s3_pins(),
        );
        assert!(
            !g.components[0]
                .connections
                .iter()
                .any(|w| w.connection.role == "signal"),
            "picked one of two conflicting values"
        );
        assert!(
            codes(&g).contains(&"guess.ambiguous_symbol"),
            "{:?}",
            g.findings
        );
    }

    #[test]
    fn a_reserved_pin_is_refused_rather_than_wired() {
        // GPIO19/20 are the S3's USB pins and are marked reserved.
        let g = guess_src(
            "#include <DHT.h>\nDHT dht(19, DHT22);\n",
            "esp32-s3-devkitc-1",
            &s3_pins(),
        );
        assert!(
            codes(&g).contains(&"guess.pin_reserved"),
            "{:?}",
            g.findings
        );
        assert!(!g.components[0]
            .connections
            .iter()
            .any(|w| w.connection.role == "signal"));
    }

    #[test]
    fn a_pin_the_board_does_not_have_is_refused() {
        let g = guess_src(
            "#include <DHT.h>\nDHT dht(99, DHT22);\n",
            "esp32-s3-devkitc-1",
            &s3_pins(),
        );
        assert!(
            codes(&g).contains(&"guess.pin_not_on_board"),
            "{:?}",
            g.findings
        );
    }

    #[test]
    fn two_of_the_same_part_get_distinct_ids() {
        let g = guess_src(
            "#include <DHT.h>\nDHT a(4, DHT22);\nDHT b(5, DHT22);\n",
            "esp32-s3-devkitc-1",
            &s3_pins(),
        );
        let ids: Vec<&str> = g.components.iter().map(|c| c.item_id.as_str()).collect();
        assert_eq!(ids, vec!["dht", "dht_2"]);
    }

    #[test]
    fn an_existing_component_is_never_overwritten() {
        let mut base = circuit::default_manifest(Some("esp32-s3-devkitc-1"), "Demo");
        base.components.push(Component {
            id: "dht".into(),
            label: "my hand-wired DHT".into(),
            kind: "generic".into(),
            part_number: None,
            voltage_v: None,
            current_ma: None,
            verified: true,
            pins: vec![],
            properties: BTreeMap::new(),
        });
        let scan = cppscan::scan_file("s.ino", "#include <DHT.h>\nDHT dht(4, DHT22);\n");
        let g = guess(
            &scan,
            &board("esp32-s3-devkitc-1"),
            BoardSource::Manifest,
            &s3_pins(),
            Some(&base),
        );
        assert_eq!(g.components[0].item_id, "dht_2");
        assert!(g.components[0].conflicts_with_existing);
        assert!(codes(&g).contains(&"guess.conflicts_with_existing"));
        // And the base is untouched — this function takes it by reference and
        // returns a proposal, never an edit.
        assert_eq!(base.components.len(), 1);
        assert_eq!(base.components[0].label, "my hand-wired DHT");
    }

    #[test]
    fn a_gpio_the_manifest_already_wires_is_left_alone() {
        let mut base = circuit::default_manifest(Some("esp32-s3-devkitc-1"), "Demo");
        base.connections.push(Connection {
            id: "existing".into(),
            from: Endpoint {
                target: "board".into(),
                pin: "GPIO4".into(),
            },
            to: Endpoint {
                target: "something".into(),
                pin: "SIG".into(),
            },
            role: "signal".into(),
            voltage_v: None,
            firmware_symbol: None,
            notes: None,
        });
        let scan = cppscan::scan_file("s.ino", "#include <DHT.h>\nDHT dht(4, DHT22);\n");
        let g = guess(
            &scan,
            &board("esp32-s3-devkitc-1"),
            BoardSource::Manifest,
            &s3_pins(),
            Some(&base),
        );
        assert!(codes(&g).contains(&"guess.pin_taken"), "{:?}", g.findings);
        assert!(!g.components[0]
            .connections
            .iter()
            .any(|w| w.connection.from.pin == "GPIO4"));
    }

    #[test]
    fn no_starter_template_has_hardware_invented_for_it() {
        // The repo ships seven sketches whose wiring is known, and the correct
        // answer for every one of them is *nothing*: none includes a library
        // this table recognises. They are the negative corpus, and the
        // strongest available evidence that the engine does not fabricate.
        //
        // Four say so outright ("no wiring", "a potentiometer is enough"), and
        // the interesting three are traps:
        //   - blink       guarded `#define LED_BUILTIN 2`, wrong on the S3
        //   - analog_plot `#define ANALOG_PIN A0`, a core alias
        //   - i2c_scan    a `-1` sentinel, and a sweep over addresses 8..120
        //     that a naive address reader would turn into 112 components
        for template in crate::project::TEMPLATES {
            let src = crate::project::sketch_from_template(template.id, "Demo")
                .unwrap_or_else(|| panic!("{} did not render", template.id));
            for (board_id, pins) in [
                ("esp32-s3-devkitc-1", s3_pins()),
                ("esp32-devkitc-v4", classic_pins()),
            ] {
                let g = guess_src(&src, board_id, &pins);
                assert!(
                    g.components.is_empty(),
                    "{} on {board_id} invented {:?}",
                    template.id,
                    g.components
                        .iter()
                        .map(|c| c.item_id.as_str())
                        .collect::<Vec<_>>()
                );
                assert!(
                    g.findings.is_empty(),
                    "{} on {board_id} produced findings with nothing to find: {:?}",
                    template.id,
                    codes(&g)
                );
            }
        }
    }

    #[test]
    fn the_i2c_scanner_sweep_never_becomes_a_hundred_components() {
        // i2c_scan loops `for (addr = 8; addr < 120; addr++)` and calls
        // Wire.beginTransmission(addr). Anything that read those as device
        // addresses would propose 112 sensors.
        let src = crate::project::sketch_from_template("i2c-scan", "Demo").unwrap();
        let g = guess_src(&src, "esp32-s3-devkitc-1", &s3_pins());
        assert!(g.components.is_empty(), "{:?}", g.components);
    }

    fn dht_guess() -> CircuitGuess {
        guess_src(
            "#include <DHT.h>\nDHT dht(4, DHT22);\n",
            "esp32-s3-devkitc-1",
            &s3_pins(),
        )
    }

    #[test]
    fn accepting_nothing_returns_the_base_untouched() {
        let base = circuit::default_manifest(Some("esp32-s3-devkitc-1"), "Demo");
        let merged = merge(&dht_guess(), &base, &[]).unwrap();
        assert_eq!(merged, base);
    }

    #[test]
    fn accepting_a_component_brings_the_wires_selected_with_it() {
        let base = circuit::default_manifest(Some("esp32-s3-devkitc-1"), "Demo");
        let g = dht_guess();
        let merged = merge(&g, &base, &all_item_ids(&g)).unwrap();
        assert_eq!(merged.components.len(), 1);
        assert_eq!(merged.components[0].id, "dht");
        assert_eq!(merged.connections.len(), g.components[0].connections.len());
        // Every endpoint resolves: no wire points at a component that is not
        // there, which is what `connection.endpoint` would reject.
        for wire in &merged.connections {
            for endpoint in [&wire.from, &wire.to] {
                assert!(
                    endpoint.target == "board"
                        || merged.components.iter().any(|c| c.id == endpoint.target),
                    "dangling endpoint {endpoint:?}"
                );
            }
        }
    }

    #[test]
    fn accepting_a_component_without_its_wires_is_allowed() {
        // Ticking the part but not a wire you disagree with is a real
        // workflow; only the reverse is incoherent.
        let base = circuit::default_manifest(Some("esp32-s3-devkitc-1"), "Demo");
        let g = dht_guess();
        let merged = merge(&g, &base, &["dht".to_string()]).unwrap();
        assert_eq!(merged.components.len(), 1);
        assert!(merged.connections.is_empty());
    }

    #[test]
    fn accepting_a_wire_without_its_component_is_refused() {
        let base = circuit::default_manifest(Some("esp32-s3-devkitc-1"), "Demo");
        let g = dht_guess();
        let wire = g.components[0].connections[0].item_id.clone();
        let err = merge(&g, &base, &[wire.clone()]).unwrap_err().to_string();
        assert!(err.contains(&wire), "{err}");
        assert!(err.contains("dht"), "{err}");
    }

    #[test]
    fn merging_never_mutates_the_base() {
        let mut base = circuit::default_manifest(Some("esp32-s3-devkitc-1"), "Demo");
        base.components.push(Component {
            id: "mine".into(),
            label: "hand written".into(),
            kind: "generic".into(),
            part_number: None,
            voltage_v: None,
            current_ma: None,
            verified: true,
            pins: vec![],
            properties: BTreeMap::new(),
        });
        let before = base.clone();
        let g = dht_guess();
        let merged = merge(&g, &base, &all_item_ids(&g)).unwrap();
        assert_eq!(base, before, "merge mutated its input");
        // The hand-written component survives, first and unchanged.
        assert_eq!(merged.components[0], before.components[0]);
        assert_eq!(merged.board, before.board, "merge changed the board");
    }

    #[test]
    fn the_todo_list_is_the_validator_speaking_not_a_hand_written_message() {
        // A proposed part is unverified by construction, so accepting it
        // leaves the build blocked until a human confirms. That is the
        // feature working, and the user is shown it before accepting.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("s.ino"), "void setup() {}\n").unwrap();
        let base = circuit::default_manifest(Some("esp32-s3-devkitc-1"), "Demo");
        let mut g = dht_guess();
        annotate_todos(&mut g, dir.path(), &base, Some("esp32:esp32:esp32s3"));
        let codes: Vec<&str> = g.components[0]
            .todo
            .iter()
            .map(|d| d.code.as_str())
            .collect();
        assert!(codes.contains(&"component.unverified"), "{codes:?}");
        assert!(
            g.components[0]
                .todo
                .iter()
                .all(|d| d.severity == Severity::Error),
            "a todo is something that blocks: {:?}",
            g.components[0].todo
        );
    }

    #[test]
    fn an_led_part_is_told_it_still_needs_its_series_resistor() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("s.ino"), "void setup() {}\n").unwrap();
        let base = circuit::default_manifest(Some("esp32-s3-devkitc-1"), "Demo");
        let mut g = guess_src(
            "#include <Adafruit_NeoPixel.h>\nAdafruit_NeoPixel strip(60, 5, NEO_GRB);\n",
            "esp32-s3-devkitc-1",
            &s3_pins(),
        );
        annotate_todos(&mut g, dir.path(), &base, Some("esp32:esp32:esp32s3"));
        let codes: Vec<&str> = g.components[0]
            .todo
            .iter()
            .map(|d| d.code.as_str())
            .collect();
        assert!(codes.contains(&"safety.led_resistor"), "{codes:?}");
    }

    #[test]
    fn the_same_input_guesses_identically_twice() {
        let src = "#include <DHT.h>\n#include <Adafruit_BME280.h>\n\
                   DHT dht(4, DHT22);\nAdafruit_BME280 bme;\n";
        let a = guess_src(src, "esp32-s3-devkitc-1", &s3_pins());
        let b = guess_src(src, "esp32-s3-devkitc-1", &s3_pins());
        assert_eq!(a, b);
    }
}
