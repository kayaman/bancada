use em_core::{Mesh, Vec3};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PartTransform {
    #[serde(default)]
    pub translation: [f64; 3],
    #[serde(default)]
    pub rotation: [f64; 3],
}

pub type Transforms = BTreeMap<String, PartTransform>;

impl PartTransform {
    pub fn validate(&self) -> Result<(), String> {
        if self
            .translation
            .iter()
            .chain(&self.rotation)
            .any(|v| !v.is_finite() || v.abs() > 100_000.0)
        {
            return Err(
                "Position and rotation must be finite numbers between -100000 and 100000.".into(),
            );
        }
        Ok(())
    }

    pub fn apply(&self, mesh: &Mesh) -> Mesh {
        let [cx, cy, cz] = center(mesh);
        let pivot = Vec3::new(cx, cy, cz);
        let [rx, ry, rz] = self.rotation.map(f64::to_radians);
        let (sx, cx) = rx.sin_cos();
        let (sy, cy) = ry.sin_cos();
        let (sz, cz) = rz.sin_cos();
        let rotate = |p: Vec3| {
            let p = Vec3::new(p.x, p.y * cx - p.z * sx, p.y * sx + p.z * cx);
            let p = Vec3::new(p.x * cy + p.z * sy, p.y, -p.x * sy + p.z * cy);
            Vec3::new(p.x * cz - p.y * sz, p.x * sz + p.y * cz, p.z)
        };
        let offset = Vec3::new(
            self.translation[0],
            self.translation[1],
            self.translation[2],
        );
        let mut output = mesh.clone();
        for triangle in &mut output.triangles {
            for vertex in triangle {
                vertex.pos = rotate(vertex.pos - pivot) + pivot + offset;
                vertex.normal = rotate(vertex.normal);
            }
        }
        output
    }
}

pub fn center(mesh: &Mesh) -> [f64; 3] {
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for vertex in mesh.triangles.iter().flatten() {
        for (i, value) in [vertex.pos.x, vertex.pos.y, vertex.pos.z]
            .into_iter()
            .enumerate()
        {
            min[i] = min[i].min(value);
            max[i] = max[i].max(value);
        }
    }
    if mesh.triangles.is_empty() {
        return [0.0; 3];
    }
    std::array::from_fn(|i| (min[i] + max[i]) / 2.0)
}

fn path(script: &Path) -> PathBuf {
    let filename = script.file_name().unwrap_or_default().to_string_lossy();
    script
        .parent()
        .unwrap_or(Path::new("."))
        .join(".enclosure-maker")
        .join(format!("{filename}.transforms.json"))
}

pub fn load(script: &Path) -> Result<Transforms, String> {
    let path = path(script);
    match std::fs::read_to_string(&path) {
        Ok(source) => {
            let transforms: Transforms = serde_json::from_str(&source)
                .map_err(|e| format!("Could not read saved object controls: {e}"))?;
            for transform in transforms.values() {
                transform.validate()?;
            }
            Ok(transforms)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Transforms::new()),
        Err(e) => Err(format!("Could not read saved object controls: {e}")),
    }
}

pub fn save(script: &Path, transforms: &Transforms) -> Result<(), String> {
    let path = path(script);
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    let temporary = path.with_extension("json.tmp");
    let source = serde_json::to_string_pretty(transforms).map_err(|e| e.to_string())?;
    std::fs::write(&temporary, source).map_err(|e| e.to_string())?;
    std::fs::rename(temporary, path).map_err(|e| format!("Could not save object controls: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use em_core::Vertex;

    #[test]
    fn rotates_about_the_part_center_then_translates() {
        let mesh = Mesh {
            triangles: vec![[
                Vertex::new(Vec3::new(10.0, 0.0, 0.0), Vec3::ZERO),
                Vertex::new(Vec3::new(14.0, 0.0, 0.0), Vec3::ZERO),
                Vertex::new(Vec3::new(10.0, 2.0, 0.0), Vec3::ZERO),
            ]],
        };
        let transform = PartTransform {
            translation: [3.0, 4.0, 5.0],
            rotation: [0.0, 0.0, 90.0],
        };
        let output = transform.apply(&mesh);
        let p = output.triangles[0][0].pos;
        assert!((p.x - 16.0).abs() < 1e-8);
        assert!((p.y - 3.0).abs() < 1e-8);
        assert!((p.z - 5.0).abs() < 1e-8);
        assert_eq!(mesh.triangles[0][0].pos.x, 10.0);
    }
}
