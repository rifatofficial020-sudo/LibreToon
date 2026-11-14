//! Shared document storage. Cloning a snapshot is constant time; mutation
//! copies only the containers along the edited path, never unrelated geometry.
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{
    ops::{Deref, DerefMut},
    sync::Arc,
};

#[derive(Clone, Debug, PartialEq)]
pub struct SharedVec<T>(Arc<Vec<T>>);
impl<T> SharedVec<T> {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn identity(&self) -> usize {
        Arc::as_ptr(&self.0) as usize
    }
}
impl<T> Default for SharedVec<T> {
    fn default() -> Self {
        Self(Arc::new(Vec::new()))
    }
}
impl<T> From<Vec<T>> for SharedVec<T> {
    fn from(value: Vec<T>) -> Self {
        Self(Arc::new(value))
    }
}
impl<T> FromIterator<T> for SharedVec<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        Vec::from_iter(iter).into()
    }
}
impl<T> Deref for SharedVec<T> {
    type Target = Vec<T>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl<T: Clone> DerefMut for SharedVec<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        Arc::make_mut(&mut self.0)
    }
}
impl<'a, T> IntoIterator for &'a SharedVec<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
impl<'a, T: Clone> IntoIterator for &'a mut SharedVec<T> {
    type Item = &'a mut T;
    type IntoIter = std::slice::IterMut<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}
impl<T: Serialize> Serialize for SharedVec<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.as_ref().serialize(serializer)
    }
}
impl<'de, T: Deserialize<'de>> Deserialize<'de> for SharedVec<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Vec::<T>::deserialize(deserializer).map(Self::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Document, Mark};
    #[test]
    fn snapshots_share_geometry_and_isolate_edits() {
        let mut doc = Document::default();
        doc.layers[0].frames[0].push(Mark {
            points: vec![[10., 20.]; 10_000].into(),
            color: [0, 0, 0, 255],
            width: 4.,
            rectangle: false,
            ..Mark::default()
        });
        doc.layers[0].frames[1] = doc.layers[0].frames[0].clone();
        let before = doc.clone();
        assert!(Arc::ptr_eq(&before.layers.0, &doc.layers.0));
        doc.layers[0].frames[0][0].points[0] = [30., 40.];
        assert_eq!(before.layers[0].frames[0][0].points[0], [10., 20.]);
        assert_eq!(doc.layers[0].frames[1][0].points[0], [10., 20.]);
        assert!(Arc::ptr_eq(
            &before.layers[0].frames[1].0,
            &doc.layers[0].frames[1].0
        ));
        let restored: Document =
            serde_json::from_slice(&serde_json::to_vec(&doc).unwrap()).unwrap();
        assert_eq!(restored.layers[0].frames[0][0].points[0], [30., 40.]);
        assert!(restored.validate().is_ok());
    }
    #[test]
    fn legacy_json_schema_is_unchanged() {
        let legacy = r#"{"version":1,"fps":12,"layers":[{"name":"Capa 1","visible":true,"frames":[[{"points":[[1.0,2.0]],"color":[0,0,0,255],"width":4.0,"rectangle":false}]]}]}"#;
        let doc: Document = serde_json::from_str(legacy).unwrap();
        assert!(doc.validate().is_ok());
        assert_eq!(
            serde_json::to_value(&doc).unwrap(),
            serde_json::from_str::<serde_json::Value>(legacy).unwrap()
        );
    }
    #[test]
    #[ignore = "manual release benchmark"]
    fn benchmark_snapshots() {
        let mut doc = Document::default();
        let mark = Mark {
            points: vec![[10., 20.]; 100_000].into(),
            color: [0, 0, 0, 255],
            width: 4.,
            rectangle: false,
            ..Mark::default()
        };
        for frame in &mut doc.layers[0].frames {
            frame.push(mark.clone());
        }
        let start = std::time::Instant::now();
        for _ in 0..100_000 {
            std::hint::black_box(doc.clone());
        }
        let shared = start.elapsed();
        let start = std::time::Instant::now();
        // Reproduce the former deep geometry copies (24 * 100000 points).
        for _ in 0..100 {
            let copy: Vec<Vec<Vec<[f32; 2]>>> = doc.layers[0]
                .frames
                .iter()
                .map(|frame| frame.iter().map(|mark| mark.points.to_vec()).collect())
                .collect();
            std::hint::black_box(copy);
        }
        let deep = start.elapsed();
        eprintln!(
            "snapshot: shared={:?}/op (100000 iterations), former geometry copies={:?}/op (100 iterations), logical points=2400000",
            shared / 100000,
            deep / 100
        );
    }
}
