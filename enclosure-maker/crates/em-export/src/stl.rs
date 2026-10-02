use em_core::Mesh;
use std::io::Write as _;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("zip error: {0}")]
    Zip(#[from] zip::result::ZipError),
}

/// Serializes a mesh to the binary STL format: an 80-byte header, a u32
/// triangle count, then 50 bytes per triangle (a computed face normal, the
/// three vertex positions, all little-endian f32, plus a zero attribute
/// count).
pub fn to_binary_stl(mesh: &Mesh) -> Vec<u8> {
    let mut out = Vec::with_capacity(84 + mesh.triangles.len() * 50);
    out.extend_from_slice(&[0u8; 80]);
    out.extend_from_slice(&(mesh.triangles.len() as u32).to_le_bytes());

    for tri in &mesh.triangles {
        let [a, b, c] = *tri;
        let normal = (b.pos - a.pos).cross(c.pos - a.pos).normalize();

        for component in [normal.x, normal.y, normal.z] {
            out.extend_from_slice(&(component as f32).to_le_bytes());
        }
        for vertex in [a, b, c] {
            for component in [vertex.pos.x, vertex.pos.y, vertex.pos.z] {
                out.extend_from_slice(&(component as f32).to_le_bytes());
            }
        }
        out.extend_from_slice(&0u16.to_le_bytes());
    }

    out
}

pub fn write_binary_stl(mesh: &Mesh, path: impl AsRef<Path>) -> Result<(), ExportError> {
    let bytes = to_binary_stl(mesh);
    let mut file = std::fs::File::create(path)?;
    file.write_all(&bytes)?;
    Ok(())
}

/// Serializes a mesh to the classic ASCII STL text format. Larger than
/// binary and slower to parse, but human-readable and diffable.
pub fn to_ascii_stl(mesh: &Mesh, solid_name: &str) -> String {
    let mut out = String::with_capacity(mesh.triangles.len() * 200);
    out.push_str(&format!("solid {solid_name}\n"));
    for tri in &mesh.triangles {
        let [a, b, c] = *tri;
        let normal = (b.pos - a.pos).cross(c.pos - a.pos).normalize();
        out.push_str(&format!(
            "facet normal {:e} {:e} {:e}\n",
            normal.x, normal.y, normal.z
        ));
        out.push_str("  outer loop\n");
        for vertex in [a, b, c] {
            out.push_str(&format!(
                "    vertex {:e} {:e} {:e}\n",
                vertex.pos.x, vertex.pos.y, vertex.pos.z
            ));
        }
        out.push_str("  endloop\n");
        out.push_str("endfacet\n");
    }
    out.push_str(&format!("endsolid {solid_name}\n"));
    out
}

pub fn write_ascii_stl(mesh: &Mesh, solid_name: &str, path: impl AsRef<Path>) -> Result<(), ExportError> {
    let text = to_ascii_stl(mesh, solid_name);
    std::fs::write(path, text)?;
    Ok(())
}
