mod stl;
mod threemf;

pub use stl::{to_ascii_stl, to_binary_stl, write_ascii_stl, write_binary_stl, ExportError};
pub use threemf::{to_3mf, write_3mf};

#[cfg(test)]
mod tests {
    use super::*;
    use em_core::{Mesh, Vec3, Vertex};

    fn single_triangle_mesh() -> Mesh {
        let v = |x: f64, y: f64, z: f64| Vertex::new(Vec3::new(x, y, z), Vec3::new(0.0, 0.0, 1.0));
        Mesh {
            triangles: vec![[v(0.0, 0.0, 0.0), v(1.0, 0.0, 0.0), v(0.0, 1.0, 0.0)]],
        }
    }

    fn two_triangle_mesh() -> Mesh {
        let v = |x: f64, y: f64, z: f64| Vertex::new(Vec3::new(x, y, z), Vec3::new(0.0, 0.0, 1.0));
        Mesh {
            triangles: vec![
                [v(0.0, 0.0, 0.0), v(1.0, 0.0, 0.0), v(0.0, 1.0, 0.0)],
                [v(1.0, 0.0, 0.0), v(1.0, 1.0, 0.0), v(0.0, 1.0, 0.0)],
            ],
        }
    }

    #[test]
    fn round_trip_binary_stl() {
        let mesh = single_triangle_mesh();
        let bytes = to_binary_stl(&mesh);

        assert_eq!(bytes.len(), 80 + 4 + 50);
        let tri_count = u32::from_le_bytes(bytes[80..84].try_into().unwrap());
        assert_eq!(tri_count, 1);

        let read_f32 = |offset: usize| f32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
        // Normal should be +Z for this CCW XY-plane triangle.
        assert!((read_f32(84) - 0.0).abs() < 1e-6);
        assert!((read_f32(88) - 0.0).abs() < 1e-6);
        assert!((read_f32(92) - 1.0).abs() < 1e-6);

        // First vertex position (at offset 84 + 12).
        assert!((read_f32(96) - 0.0).abs() < 1e-6);
        assert!((read_f32(100) - 0.0).abs() < 1e-6);
        assert!((read_f32(104) - 0.0).abs() < 1e-6);
    }

    #[test]
    fn round_trip_ascii_stl() {
        let mesh = two_triangle_mesh();
        let text = to_ascii_stl(&mesh, "test_part");

        assert!(text.starts_with("solid test_part\n"));
        assert!(text.trim_end().ends_with("endsolid test_part"));
        assert_eq!(text.matches("facet normal").count(), 2);
        assert_eq!(text.matches("vertex").count(), 6);
    }

    #[test]
    fn write_3mf_produces_a_valid_zip_with_expected_structure() {
        let dir = std::env::temp_dir().join(format!("em-export-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.3mf");

        let parts = vec![
            ("base".to_string(), single_triangle_mesh()),
            ("lid".to_string(), two_triangle_mesh()),
        ];
        write_3mf(&parts, &path).expect("3mf write should succeed");

        let file = std::fs::File::open(&path).unwrap();
        let mut archive = zip::ZipArchive::new(file).expect("should be a valid zip");

        let mut names: Vec<String> = (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().to_string())
            .collect();
        names.sort();
        assert_eq!(names, vec!["3D/3dmodel.model", "[Content_Types].xml", "_rels/.rels"]);

        let mut model_xml = String::new();
        std::io::Read::read_to_string(&mut archive.by_name("3D/3dmodel.model").unwrap(), &mut model_xml).unwrap();
        assert_eq!(model_xml.matches("<object ").count(), 2, "one <object> per part");
        assert_eq!(model_xml.matches("<item ").count(), 2, "one <item> per object, per part");
        assert!(model_xml.contains("core/2015/02"));

        let mut rels_xml = String::new();
        std::io::Read::read_to_string(&mut archive.by_name("_rels/.rels").unwrap(), &mut rels_xml).unwrap();
        assert!(rels_xml.contains("2013/01/3dmodel"));
        assert!(rels_xml.contains("Target=\"/3D/3dmodel.model\""));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn xml_escape_handles_special_characters_in_part_names() {
        let dir = std::env::temp_dir().join(format!("em-export-test-esc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.3mf");

        let parts = vec![("<weird> & \"name\"".to_string(), single_triangle_mesh())];
        write_3mf(&parts, &path).expect("3mf write should succeed even with special characters in names");

        let file = std::fs::File::open(&path).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        let mut model_xml = String::new();
        std::io::Read::read_to_string(&mut archive.by_name("3D/3dmodel.model").unwrap(), &mut model_xml).unwrap();
        assert!(model_xml.contains("&lt;weird&gt; &amp; &quot;name&quot;"));

        std::fs::remove_dir_all(&dir).ok();
    }
}
