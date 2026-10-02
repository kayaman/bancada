use serde::Serialize;

#[derive(Serialize)]
pub struct NumberControl {
    pub id: String,
    pub label: String,
    pub group: String,
    pub value: f64,
    pub integer: bool,
    pub min: f64,
    pub max: f64,
    #[serde(skip)]
    start: usize,
    #[serde(skip)]
    end: usize,
}

struct Token<'a> {
    text: &'a str,
    start: usize,
    end: usize,
}

// Read tokens without interpreting strings or comments as code. Byte ranges
// are retained so a control changes exactly one literal in the source file.
fn tokens(source: &str) -> Vec<Token<'_>> {
    let b = source.as_bytes();
    let mut output = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_whitespace() || !b[i].is_ascii() {
            i += 1;
            continue;
        }
        if b[i..].starts_with(b"//") {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if b[i..].starts_with(b"/*") {
            i += 2;
            let mut depth = 1;
            while i < b.len() && depth > 0 {
                if b[i..].starts_with(b"/*") {
                    depth += 1;
                    i += 2;
                } else if b[i..].starts_with(b"*/") {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            continue;
        }
        let start = i;
        if matches!(b[i], b'"' | b'\'' | b'`') {
            let quote = b[i];
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i = (i + 2).min(b.len());
                } else if b[i] == quote {
                    i += 1;
                    break;
                } else {
                    i += 1;
                }
            }
        } else if b[i].is_ascii_alphabetic() || b[i] == b'_' {
            i += 1;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
        } else if b[i].is_ascii_digit() {
            i += 1;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
            if i + 1 < b.len() && b[i] == b'.' && b[i + 1].is_ascii_digit() {
                i += 1;
                while i < b.len() && b[i].is_ascii_digit() {
                    i += 1;
                }
            }
            if i < b.len() && matches!(b[i], b'e' | b'E') {
                i += 1;
                if i < b.len() && matches!(b[i], b'+' | b'-') {
                    i += 1;
                }
                while i < b.len() && b[i].is_ascii_digit() {
                    i += 1;
                }
            }
        } else {
            i += 1;
        }
        output.push(Token {
            text: &source[start..i],
            start,
            end: i,
        });
    }
    output
}

fn literal(tokens: &[Token<'_>]) -> Option<(usize, usize, f64, bool)> {
    let (sign, number) = match tokens {
        [number] => (1.0, number),
        [sign, number] if sign.text == "-" || sign.text == "+" => {
            (if sign.text == "-" { -1.0 } else { 1.0 }, number)
        }
        _ => return None,
    };
    let value = number.text.parse::<f64>().ok()? * sign;
    if !value.is_finite() {
        return None;
    }
    Some((
        tokens[0].start,
        number.end,
        value,
        !number.text.contains(['.', 'e', 'E']),
    ))
}

fn control(
    source: &str,
    tokens: &[Token<'_>],
    label: &str,
    group: &str,
    min: f64,
    max: f64,
) -> Option<NumberControl> {
    let (start, end, value, integer) = literal(tokens)?;
    let line = source[..start].bytes().filter(|b| *b == b'\n').count() + 1;
    Some(NumberControl {
        id: format!("{start}:{end}"),
        label: format!("{label} · line {line}"),
        group: group.into(),
        value,
        integer,
        min,
        max,
        start,
        end,
    })
}

pub fn numbers(source: &str) -> Vec<NumberControl> {
    let tokens = tokens(source);
    let mut output = Vec::new();
    for i in 0..tokens.len() {
        if matches!(tokens[i].text, "let" | "const")
            && tokens.get(i + 2).is_some_and(|t| t.text == "=")
        {
            if let Some(end) = (i + 3..tokens.len()).find(|j| tokens[*j].text == ";") {
                if let Some(control) = control(
                    source,
                    &tokens[i + 3..end],
                    &tokens[i + 1].text.replace('_', " "),
                    "Named dimensions",
                    -100000.0,
                    100000.0,
                ) {
                    output.push(control);
                }
            }
        }
        if !tokens.get(i + 1).is_some_and(|t| t.text == "(")
            || (i > 0 && tokens[i - 1].text == "fn")
        {
            continue;
        }
        let labels: &[(&str, f64, f64)] = match tokens[i].text {
            "screw_boss" => &[
                ("Boss thread (M)", 2.0, 4.0),
                ("Boss height (mm)", 0.001, 100000.0),
                ("Boss gussets", 2.0, 4.0),
                ("Boss wall (mm)", 0.001, 100000.0),
            ],
            "pcb_standoff" => &[
                ("Standoff thread (M)", 2.0, 4.0),
                ("Standoff height (mm)", 0.001, 100000.0),
                ("Standoff wall (mm)", 0.001, 100000.0),
            ],
            "heat_set_bore" | "hex_nut_trap" => &[
                ("Fastener thread (M)", 2.0, 4.0),
                ("Nut trap extra depth (mm)", 0.0, 100000.0),
            ],
            "cuboid" => &[
                ("Box width (mm)", 0.001, 100000.0),
                ("Box depth (mm)", 0.001, 100000.0),
                ("Box height (mm)", 0.001, 100000.0),
            ],
            "rounded_box" | "chamfered_box" => &[
                ("Box width (mm)", 0.001, 100000.0),
                ("Box depth (mm)", 0.001, 100000.0),
                ("Box height (mm)", 0.001, 100000.0),
                ("Corner size (mm)", 0.0, 100000.0),
            ],
            "cylinder" => &[
                ("Cylinder radius (mm)", 0.001, 100000.0),
                ("Cylinder height (mm)", 0.001, 100000.0),
            ],
            "sphere" => &[("Sphere radius (mm)", 0.001, 100000.0)],
            _ => continue,
        };
        let group = if matches!(
            tokens[i].text,
            "screw_boss" | "pcb_standoff" | "heat_set_bore" | "hex_nut_trap"
        ) {
            "Hardware"
        } else {
            "Shapes"
        };
        let mut depth = 0;
        let mut start = i + 2;
        let mut argument = 0;
        for j in start..tokens.len() {
            let text = tokens[j].text;
            if (text == "," || text == ")") && depth == 0 {
                if let Some((label, min, max)) = labels.get(argument) {
                    if let Some(control) =
                        control(source, &tokens[start..j], label, group, *min, *max)
                    {
                        output.push(control);
                    }
                }
                if text == ")" {
                    break;
                }
                argument += 1;
                start = j + 1;
            } else if matches!(text, "(" | "[" | "{") {
                depth += 1;
            } else if matches!(text, ")" | "]" | "}") {
                depth -= 1;
            }
        }
    }
    output
}

pub fn edit(source: &str, id: &str, value: f64) -> Result<String, String> {
    let control = numbers(source)
        .into_iter()
        .find(|control| control.id == id)
        .ok_or("That dimension is no longer available. Refresh the controls.")?;
    if !value.is_finite()
        || value < control.min
        || value > control.max
        || (control.integer && value.fract() != 0.0)
    {
        return Err(format!(
            "Enter {}a value from {} to {}.",
            if control.integer { "an integer: " } else { "" },
            control.min,
            control.max
        ));
    }
    let literal = if control.integer {
        format!("{value:.0}")
    } else if value.fract() == 0.0 {
        format!("{value:.1}")
    } else {
        value.to_string()
    };
    let mut edited = source.to_string();
    edited.replace_range(control.start..control.end, &literal);
    Ok(edited)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hardware_dimensions_edit_only_the_selected_literal() {
        let source = "// screw_boss(3, 9.0, 3, 8.0)\nlet height = 12.0;\nlet boss = screw_boss(3, height, 4, 2.4);\nemit(boss);";
        let controls = numbers(source);
        assert_eq!(controls.len(), 4);
        let wall = controls
            .iter()
            .find(|c| c.label.starts_with("Boss wall"))
            .unwrap();
        let edited = edit(source, &wall.id, 3.25).unwrap();
        assert_eq!(edited, source.replace("4, 2.4)", "4, 3.25)"));
        assert!(edit(
            source,
            &controls
                .iter()
                .find(|c| c.label.starts_with("Boss thread"))
                .unwrap()
                .id,
            2.5
        )
        .is_err());
    }

    #[test]
    fn ignores_comments_strings_and_numeric_expressions() {
        let source = "/* let fake = 99.0; */ let label = \"cuboid(1.0, 2.0, 3.0)\"; let w = 20.0 + 2.0; let x = -1.25; emit(cuboid(w, 12.0, 6.0));";
        let controls = numbers(source);
        assert_eq!(controls.len(), 3);
        assert!(controls.iter().any(|c| c.value == -1.25));
        assert!(controls.iter().all(|c| !c.label.starts_with("w ·")));
    }
}
