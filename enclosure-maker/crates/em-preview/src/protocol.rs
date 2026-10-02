use em_core::{Mesh, Vec3, Vertex};

/// Binary wire format sent to the browser: a u32 (LE) triangle count
/// followed by 9 LE f32s per triangle (3 vertex positions, no normals —
/// the viewer computes those from the geometry).
pub fn mesh_to_wire(mesh: &Mesh) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + mesh.triangles.len() * 36);
    out.extend_from_slice(&(mesh.triangles.len() as u32).to_le_bytes());
    for tri in &mesh.triangles {
        for vertex in tri {
            for component in [vertex.pos.x, vertex.pos.y, vertex.pos.z] {
                out.extend_from_slice(&(component as f32).to_le_bytes());
            }
        }
    }
    out
}

/// The inverse of [`mesh_to_wire`]. Vertex normals are left zero --
/// `em_export::to_binary_stl` computes its own face normal from the
/// triangle's positions and never reads the stored per-vertex normal.
pub fn wire_to_mesh(wire: &[u8]) -> Option<Mesh> {
    if wire.len() < 4 {
        return None;
    }
    let count = u32::from_le_bytes(wire[0..4].try_into().ok()?) as usize;
    let needed = 4 + count * 36;
    if wire.len() < needed {
        return None;
    }

    let mut triangles = Vec::with_capacity(count);
    let mut offset = 4;
    for _ in 0..count {
        let mut positions = [Vec3::ZERO; 3];
        for pos in &mut positions {
            let mut comp = [0.0; 3];
            for c in &mut comp {
                *c = f32::from_le_bytes(wire[offset..offset + 4].try_into().ok()?) as f64;
                offset += 4;
            }
            *pos = Vec3::new(comp[0], comp[1], comp[2]);
        }
        triangles.push(positions.map(|p| Vertex::new(p, Vec3::ZERO)));
    }
    Some(Mesh { triangles })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_round_trips_through_mesh() {
        let v = |x: f64, y: f64, z: f64| Vertex::new(Vec3::new(x, y, z), Vec3::new(0.0, 0.0, 1.0));
        let mesh = Mesh {
            triangles: vec![
                [v(0.0, 0.0, 0.0), v(1.0, 0.0, 0.0), v(0.0, 1.0, 0.0)],
                [v(1.0, 1.0, 1.0), v(2.0, 1.0, 1.0), v(1.0, 2.0, 1.0)],
            ],
        };
        let wire = mesh_to_wire(&mesh);
        let back = wire_to_mesh(&wire).unwrap();
        assert_eq!(back.triangles.len(), 2);
        for (orig, got) in mesh.triangles.iter().zip(&back.triangles) {
            for (ov, gv) in orig.iter().zip(got) {
                assert!((ov.pos.x - gv.pos.x).abs() < 1e-5);
                assert!((ov.pos.y - gv.pos.y).abs() < 1e-5);
                assert!((ov.pos.z - gv.pos.z).abs() < 1e-5);
            }
        }
    }

    #[test]
    fn truncated_wire_is_rejected() {
        assert!(wire_to_mesh(&[1, 2, 3]).is_none());
        let mut wire = 5u32.to_le_bytes().to_vec(); // claims 5 triangles
        wire.extend_from_slice(&[0u8; 36]); // but only supplies 1
        assert!(wire_to_mesh(&wire).is_none());
    }
}
