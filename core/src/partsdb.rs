//! What a library `#include` implies about the hardware on the bench.
//!
//! A sketch that includes `DHT.h` and constructs `DHT dht(4, DHT22)` is
//! evidence of a temperature sensor on GPIO 4. Turning that evidence into a
//! wiring proposal needs knowledge this repo has nowhere else: which pins the
//! part has, what they are called on its datasheet, and which rail it runs
//! from.
//!
//! That knowledge lives here, as a compiled-in table, for three reasons. It
//! needs no parser, schema or loader, so there is no runtime path a wrong
//! claim could arrive through. `git blame` is its audit trail. And a reviewer
//! reads a new part as a plain diff.
//!
//! **A wrong row here is a wrong wiring claim on every project that uses that
//! library**, so admission is deliberately expensive: an entry needs a
//! datasheet URL and the module in hand. The table ships small — five parts —
//! and grows one reviewed part at a time. Breadth is not the goal; a user can
//! always add a component by hand, and the form for doing so already exists.
//!
//! Two things are deliberately *not* here:
//!
//! - **Safety evidence.** No row carries a series resistor, a pull-up, a
//!   driver or flyback protection, and there is no field to put one in. Those
//!   are what the human confirms; inferring them is what
//!   `.agents/skills/esp32-circuit-designer` forbids.
//! - **I²C addresses.** `0x76` is a BME280 *and* a BMP280; `0x3C` is an
//!   SSD1306 *and* an SH1106. An address may be recorded as evidence when a
//!   sketch states one, but it may never select a row.

/// Which bus a part talks over — decides the connection `role` proposed for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bus {
    I2c,
    Spi,
    OneWire,
    Digital,
    Analog,
}

/// The supply a part is wired to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rail {
    V3v3,
    V5v,
}

impl Rail {
    /// The board pin id this rail names.
    pub fn pin(self) -> &'static str {
        match self {
            Rail::V3v3 => "3V3",
            Rail::V5v => "5V",
        }
    }
}

/// What in the source announces this part.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    /// An object of `class_name` is declared. `pin_arg` is the index of the
    /// constructor argument that names a GPIO, when there is one.
    ///
    /// The index is stored rather than assumed because it genuinely differs:
    /// `DHT(pin, type)` puts it first, `Adafruit_NeoPixel(count, pin, flags)`
    /// second.
    Class {
        include: &'static str,
        class_name: &'static str,
        pin_arg: Option<usize>,
        bus: Bus,
    },
}

/// Which of a part's declared pins plays each electrical role.
///
/// Stated per part rather than inferred from pin order: `DHT` lists
/// `VCC, DATA, GND` while a BME280 breakout lists `VIN, GND, SCK, SDI`, and
/// "the middle one is the signal" is true of neither reliably. Every name here
/// must appear in the recipe's `pins`, which a test enforces.
#[derive(Debug, Clone, Copy, Default)]
pub struct PinRoles {
    pub power: Option<&'static str>,
    pub ground: Option<&'static str>,
    /// The single data line of a digital, 1-Wire or analog part.
    pub signal: Option<&'static str>,
    pub sda: Option<&'static str>,
    pub scl: Option<&'static str>,
}

/// One part this module can recognise.
#[derive(Debug, Clone, Copy)]
pub struct PartRecipe {
    /// Stable id; also the stem of the proposed component id.
    pub id: &'static str,
    pub trigger: Trigger,
    /// Must be one of `circuit::catalog().component_kinds`. Enforced by test.
    pub kind: &'static str,
    pub label: &'static str,
    /// Datasheet pin names, in the order a wiring guide should list them.
    pub pins: &'static [&'static str],
    /// Which of `pins` is power, ground, data, SDA, SCL.
    pub roles: PinRoles,
    /// `None` when the part ships as both 3.3 V and 5 V modules — then no
    /// power wire is proposed at all, because guessing the rail wrong is how
    /// hardware dies.
    pub rail: Option<Rail>,
    /// `None` means unknown, and unknown stays unset. It never becomes 0,
    /// which would silently understate a current budget.
    pub current_ma: Option<u32>,
    /// Where the pin names and rail were read from. A row without a citation
    /// is a wiring claim without evidence.
    pub source_url: &'static str,
    /// What the human must check on the actual module. Rendered verbatim next
    /// to the proposal — this is the part the tool cannot do.
    pub verify: &'static str,
}

/// The recognised parts, **sorted by id** so a growing table keeps producing
/// reviewable diffs.
pub const PART_RECIPES: &[PartRecipe] = &[
    PartRecipe {
        id: "bme280",
        trigger: Trigger::Class {
            include: "Adafruit_BME280.h",
            class_name: "Adafruit_BME280",
            pin_arg: None,
            bus: Bus::I2c,
        },
        kind: "i2c_sensor",
        label: "BME280 temperature/humidity/pressure sensor",
        pins: &["VIN", "GND", "SCK", "SDI"],
        // Adafruit's silkscreen: SCK carries I2C clock, SDI carries data.
        roles: PinRoles {
            power: Some("VIN"),
            ground: Some("GND"),
            signal: None,
            sda: Some("SDI"),
            scl: Some("SCK"),
        },
        // Breakouts differ: a bare BME280 is a 3.3 V part, while regulated
        // modules (GY-BME280 with an AMS1117) need 5 V in. Proposing either
        // would be wrong half the time, so no power wire is proposed.
        rail: None,
        current_ma: None,
        source_url: "https://www.bosch-sensortec.com/media/boschsensortec/downloads/datasheets/bst-bme280-ds002.pdf",
        verify: "Check whether your breakout has a voltage regulator: a bare BME280 takes 3.3 V, a regulated module usually needs 5 V. Confirm the I2C address (0x76 or 0x77) and whether the board carries its own pull-ups.",
    },
    PartRecipe {
        id: "dht",
        trigger: Trigger::Class {
            include: "DHT.h",
            class_name: "DHT",
            pin_arg: Some(0),
            bus: Bus::Digital,
        },
        kind: "generic",
        label: "DHT temperature/humidity sensor",
        pins: &["VCC", "DATA", "GND"],
        roles: PinRoles {
            power: Some("VCC"),
            ground: Some("GND"),
            signal: Some("DATA"),
            sda: None,
            scl: None,
        },
        rail: Some(Rail::V3v3),
        current_ma: None,
        source_url: "https://cdn-shop.adafruit.com/datasheets/Digital+humidity+and+temperature+sensor+AM2302.pdf",
        verify: "DHT11 and DHT22 share this library but not their timing or accuracy — confirm which you have. The data line needs a pull-up (often already on the breakout); bare sensors need one added.",
    },
    PartRecipe {
        id: "ds18b20",
        trigger: Trigger::Class {
            include: "OneWire.h",
            class_name: "OneWire",
            pin_arg: Some(0),
            bus: Bus::OneWire,
        },
        kind: "generic",
        label: "1-Wire bus (DS18B20 or similar)",
        pins: &["VDD", "DQ", "GND"],
        roles: PinRoles {
            power: Some("VDD"),
            ground: Some("GND"),
            signal: Some("DQ"),
            sda: None,
            scl: None,
        },
        rail: Some(Rail::V3v3),
        current_ma: None,
        source_url: "https://www.analog.com/media/en/technical-documentation/data-sheets/ds18b20.pdf",
        verify: "1-Wire needs a pull-up on the data line (4.7 kΩ is typical) — check whether your module already has one. Confirm the part is powered rather than in parasitic mode, and that more than one device on the bus is intended if you have several.",
    },
    PartRecipe {
        id: "neopixel",
        trigger: Trigger::Class {
            include: "Adafruit_NeoPixel.h",
            class_name: "Adafruit_NeoPixel",
            // Adafruit_NeoPixel(uint16_t n, int16_t pin, neoPixelType type)
            pin_arg: Some(1),
            bus: Bus::Digital,
        },
        kind: "led",
        label: "NeoPixel / WS2812 addressable LEDs",
        pins: &["5V", "DIN", "GND"],
        roles: PinRoles {
            power: Some("5V"),
            ground: Some("GND"),
            signal: Some("DIN"),
            sda: None,
            scl: None,
        },
        rail: Some(Rail::V5v),
        // Current is entirely a function of how many pixels are lit and how
        // brightly — up to ~60 mA per pixel at full white. Any single number
        // here would be a fiction.
        current_ma: None,
        source_url: "https://cdn-shop.adafruit.com/datasheets/WS2812B.pdf",
        verify: "A 3.3 V data line into a 5 V-powered strip is marginal and often needs a level shifter. Adafruit also recommend ~330 Ω in series with the data line and a 1000 µF capacitor across the supply. Anything beyond a few pixels needs its own 5 V supply rather than the board's.",
    },
    PartRecipe {
        id: "ssd1306",
        trigger: Trigger::Class {
            include: "Adafruit_SSD1306.h",
            class_name: "Adafruit_SSD1306",
            pin_arg: None,
            bus: Bus::I2c,
        },
        kind: "i2c_sensor",
        label: "SSD1306 OLED display",
        pins: &["VCC", "GND", "SCL", "SDA"],
        roles: PinRoles {
            power: Some("VCC"),
            ground: Some("GND"),
            signal: None,
            sda: Some("SDA"),
            scl: Some("SCL"),
        },
        rail: Some(Rail::V3v3),
        current_ma: None,
        source_url: "https://cdn-shop.adafruit.com/datasheets/SSD1306.pdf",
        verify: "Confirm the module's address (0x3C or 0x3D) and that it is an SSD1306 rather than an SH1106, which is pin-compatible but needs a different driver. Check the module accepts 3.3 V — most 0.96\" boards do, some only 5 V.",
    },
];

/// The recipe a class name announces, if any.
pub fn by_class(class_name: &str) -> Option<&'static PartRecipe> {
    PART_RECIPES.iter().find(|r| match r.trigger {
        Trigger::Class { class_name: c, .. } => c == class_name,
    })
}

/// The header a recipe is announced by.
pub fn include_of(recipe: &PartRecipe) -> &'static str {
    match recipe.trigger {
        Trigger::Class { include, .. } => include,
    }
}

/// The bus a recipe talks over.
pub fn bus_of(recipe: &PartRecipe) -> Bus {
    match recipe.trigger {
        Trigger::Class { bus, .. } => bus,
    }
}

/// The constructor argument index naming a GPIO, if this part has one.
pub fn pin_arg_of(recipe: &PartRecipe) -> Option<usize> {
    match recipe.trigger {
        Trigger::Class { pin_arg, .. } => pin_arg,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::circuit;

    #[test]
    fn every_recipe_names_a_kind_the_catalog_knows() {
        // A kind outside the catalog produces a component the circuit editor
        // cannot render a template for.
        let kinds: Vec<String> = circuit::catalog()
            .component_kinds
            .iter()
            .map(|t| t.kind.clone())
            .collect();
        for r in PART_RECIPES {
            assert!(
                kinds.contains(&r.kind.to_string()),
                "{} has kind {:?}, not one of {kinds:?}",
                r.id,
                r.kind
            );
        }
    }

    #[test]
    fn every_recipe_cites_a_datasheet_and_says_what_to_verify() {
        for r in PART_RECIPES {
            assert!(
                r.source_url.starts_with("https://"),
                "{} has no citation",
                r.id
            );
            assert!(!r.verify.is_empty(), "{} says nothing to verify", r.id);
            assert!(!r.label.is_empty(), "{} has no label", r.id);
            assert!(!r.pins.is_empty(), "{} declares no pins", r.id);
        }
    }

    #[test]
    fn recipe_ids_are_unique_and_sorted() {
        // Sorted so that adding the hundredth part still produces a diff a
        // reviewer can read.
        let ids: Vec<&str> = PART_RECIPES.iter().map(|r| r.id).collect();
        let mut expected = ids.clone();
        expected.sort_unstable();
        expected.dedup();
        assert_eq!(ids, expected, "PART_RECIPES must be sorted by id, no dupes");
        for id in &ids {
            assert!(
                id.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()),
                "{id} is not a lowercase id"
            );
        }
    }

    #[test]
    fn a_pin_argument_index_is_within_the_declared_pins() {
        for r in PART_RECIPES {
            if let Some(index) = pin_arg_of(r) {
                // A constructor's pin argument only makes sense for a part
                // that has a signal pin to attach it to.
                assert!(
                    r.pins.len() > 1,
                    "{} names a pin argument but declares {:?}",
                    r.id,
                    r.pins
                );
                assert!(index < 8, "{} has an implausible pin_arg {index}", r.id);
            }
        }
    }

    #[test]
    fn every_named_role_pin_is_one_the_recipe_declares() {
        // A role naming a pin that does not exist produces a connection whose
        // endpoint fails `component.pin_unknown` — a proposal that cannot be
        // accepted.
        for r in PART_RECIPES {
            for (role, pin) in [
                ("power", r.roles.power),
                ("ground", r.roles.ground),
                ("signal", r.roles.signal),
                ("sda", r.roles.sda),
                ("scl", r.roles.scl),
            ] {
                let Some(pin) = pin else { continue };
                assert!(
                    r.pins.contains(&pin),
                    "{} names {role} pin {pin:?}, not in {:?}",
                    r.id,
                    r.pins
                );
            }
        }
    }

    #[test]
    fn a_part_with_a_rail_names_both_a_power_and_a_ground_pin() {
        // `power.missing_ground` is an error: a component wired to a rail with
        // no ground is invalid. Proposing power without ground would author
        // that error ourselves.
        for r in PART_RECIPES {
            if r.rail.is_some() {
                assert!(r.roles.power.is_some(), "{} has a rail, no power pin", r.id);
                assert!(
                    r.roles.ground.is_some(),
                    "{} has a rail, no ground pin",
                    r.id
                );
            }
        }
    }

    #[test]
    fn each_bus_names_the_pins_that_bus_needs() {
        for r in PART_RECIPES {
            match bus_of(r) {
                Bus::I2c => {
                    assert!(r.roles.sda.is_some(), "{} is I2C without SDA", r.id);
                    assert!(r.roles.scl.is_some(), "{} is I2C without SCL", r.id);
                }
                Bus::Digital | Bus::OneWire | Bus::Analog => {
                    assert!(
                        r.roles.signal.is_some(),
                        "{} has no signal pin to attach its GPIO to",
                        r.id
                    );
                }
                Bus::Spi => {}
            }
        }
    }

    #[test]
    fn no_recipe_carries_safety_evidence() {
        // The struct has no field for these by construction; this test exists
        // so that adding one is a deliberate act that breaks a named
        // assertion rather than a quiet widening.
        for r in PART_RECIPES {
            let blob = format!("{r:?}").to_lowercase();
            for forbidden in [
                "series_resistor",
                "pullups",
                "flyback",
                "driver:",
                "verified:",
            ] {
                assert!(
                    !blob.contains(forbidden),
                    "{} carries {forbidden}, which only a human may assert",
                    r.id
                );
            }
        }
    }

    #[test]
    fn no_trigger_selects_a_part_by_i2c_address() {
        // 0x76 is a BME280 and a BMP280; 0x3C is an SSD1306 and an SH1106.
        // An address is evidence, never an identification.
        for r in PART_RECIPES {
            let include = include_of(r);
            assert!(!include.contains("0x"), "{} triggers on an address", r.id);
        }
    }

    #[test]
    fn a_class_resolves_to_its_recipe_and_an_unknown_one_does_not() {
        assert_eq!(by_class("DHT").map(|r| r.id), Some("dht"));
        assert_eq!(
            by_class("Adafruit_NeoPixel").map(|r| r.id),
            Some("neopixel")
        );
        assert_eq!(pin_arg_of(by_class("DHT").unwrap()), Some(0));
        // The NeoPixel pin is the *second* argument — the reason the index is
        // stored rather than assumed.
        assert_eq!(pin_arg_of(by_class("Adafruit_NeoPixel").unwrap()), Some(1));
        assert!(by_class("SomeoneElsesSensor").is_none());
        assert!(by_class("String").is_none());
    }

    #[test]
    fn an_i2c_part_declares_no_constructor_pin() {
        // I2C parts sit on a shared bus; their pins come from Wire.begin or
        // the core's defaults, never from a constructor argument.
        for r in PART_RECIPES {
            if bus_of(r) == Bus::I2c {
                assert_eq!(pin_arg_of(r), None, "{} is I2C with a pin argument", r.id);
            }
        }
    }
}
