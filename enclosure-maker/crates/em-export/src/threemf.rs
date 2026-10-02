use em_core::{Mesh, Vec3};
use std::collections::HashMap;
use std::io::{Seek, Write};
use std::path::Path;

use crate::stl::ExportError;

/// Builds a minimal, multi-object 3MF (a ZIP containing the required
/// OPC/3MF package structure) with one `<object>` per `(name, mesh)` entry,
/// each also referenced from `<build>` so every part actually shows up as a
/// printable object in a slicer. Exact required strings (content types,
/// relationship type, core namespace) were verified against a real
/// Bambu Studio `.3mf` file rather than assumed.
pub fn to_3mf(parts: &[(String, Mesh)]) -> Result<Vec<u8>, ExportError> {
    let mut cursor = std::io::Cursor::new(Vec::new());
    write_3mf_to(parts, &mut cursor)?;
    Ok(cursor.into_inner())
}

pub fn write_3mf(parts: &[(String, Mesh)], path: impl AsRef<Path>) -> Result<(), ExportError> {
    let bytes = to_3mf(parts)?;
    std::fs::write(path, bytes)?;
    Ok(())
}

fn write_3mf_to(parts: &[(String, Mesh)], writer: impl Write + Seek) -> Result<(), ExportError> {
    let mut zip = zip::ZipWriter::new(writer);
    let options: zip::write::FileOptions<'_, ()> =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    zip.start_file("[Content_Types].xml", options)?;
    zip.write_all(CONTENT_TYPES.as_bytes())?;

    zip.start_file("_rels/.rels", options)?;
    zip.write_all(RELS.as_bytes())?;

    zip.start_file("3D/3dmodel.model", options)?;
    zip.write_all(build_model_xml(parts).as_bytes())?;

    zip.finish()?;
    Ok(())
}

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="model" ContentType="application/vnd.ms-package.3dmanufacturing-3dmodel+xml"/>
</Types>
"#;

const RELS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rel0" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel" Target="/3D/3dmodel.model"/>
</Relationships>
"#;

fn build_model_xml(parts: &[(String, Mesh)]) -> String {
    let mut resources = String::new();
    let mut build = String::new();

    for (i, (name, mesh)) in parts.iter().enumerate() {
        let id = i + 1;
        let (vertices, triangles) = index_mesh(mesh);

        resources.push_str(&format!(
            "    <object id=\"{id}\" name=\"{}\" type=\"model\">\n      <mesh>\n        <vertices>\n",
            xml_escape(name)
        ));
        for v in &vertices {
            resources.push_str(&format!(
                "          <vertex x=\"{}\" y=\"{}\" z=\"{}\"/>\n",
                v.x, v.y, v.z
            ));
        }
        resources.push_str("        </vertices>\n        <triangles>\n");
        for t in &triangles {
            resources.push_str(&format!(
                "          <triangle v1=\"{}\" v2=\"{}\" v3=\"{}\"/>\n",
                t[0], t[1], t[2]
            ));
        }
        resources.push_str("        </triangles>\n      </mesh>\n    </object>\n");

        build.push_str(&format!("    <item objectid=\"{id}\"/>\n"));
    }

    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<model unit="millimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02">
  <resources>
{resources}  </resources>
  <build>
{build}  </build>
</model>
"#
    )
}

/// Deduplicates a flat triangle-soup mesh into indexed (vertices, triangles)
/// form, per-object, 0-based -- what 3MF's `<vertices>`/`<triangles>` need,
/// as opposed to STL's independent-copy-per-corner format. Positions are
/// quantized before hashing so near-identical floating point vertices from
/// CSG operations collapse to one shared vertex.
fn index_mesh(mesh: &Mesh) -> (Vec<Vec3>, Vec<[u32; 3]>) {
    const QUANTUM: f64 = 1e6; // 1e-6 mm precision

    let quantize = |v: Vec3| -> (i64, i64, i64) {
        (
            (v.x * QUANTUM).round() as i64,
            (v.y * QUANTUM).round() as i64,
            (v.z * QUANTUM).round() as i64,
        )
    };

    let mut index_of: HashMap<(i64, i64, i64), u32> = HashMap::new();
    let mut vertices = Vec::new();
    let mut triangles = Vec::with_capacity(mesh.triangles.len());

    for tri in &mesh.triangles {
        let mut idx = [0u32; 3];
        for (slot, vertex) in idx.iter_mut().zip(tri.iter()) {
            let key = quantize(vertex.pos);
            *slot = *index_of.entry(key).or_insert_with(|| {
                vertices.push(vertex.pos);
                (vertices.len() - 1) as u32
            });
        }
        triangles.push(idx);
    }

    (vertices, triangles)
}

fn xml_escape(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '&' => "&amp;".to_string(),
            '<' => "&lt;".to_string(),
            '>' => "&gt;".to_string(),
            '"' => "&quot;".to_string(),
            '\'' => "&apos;".to_string(),
            other => other.to_string(),
        })
        .collect()
}
