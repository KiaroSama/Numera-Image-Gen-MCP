use super::{JsString, Value};

pub trait ObjectKey {
    fn matches(&self, key: &JsString) -> bool;
}
impl ObjectKey for str {
    fn matches(&self, key: &JsString) -> bool {
        key.eq(self)
    }
}
impl ObjectKey for String {
    fn matches(&self, key: &JsString) -> bool {
        key.eq(self.as_str())
    }
}
impl ObjectKey for JsString {
    fn matches(&self, key: &JsString) -> bool {
        key == self
    }
}
impl<T: ObjectKey + ?Sized> ObjectKey for &T {
    fn matches(&self, key: &JsString) -> bool {
        (*self).matches(key)
    }
}

/// Ordered UTF-16 map. Keys are unique; assignment never changes first insertion position.
#[derive(Default, Debug)]
pub struct Object(pub(super) Vec<(JsString, Value)>);
impl Object {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn clear(&mut self) {
        self.0.clear();
    }
    pub fn get<K: ObjectKey + ?Sized>(&self, key: &K) -> Option<&Value> {
        self.0.iter().find(|(k, _)| key.matches(k)).map(|(_, v)| v)
    }
    pub fn get_mut<K: ObjectKey + ?Sized>(&mut self, key: &K) -> Option<&mut Value> {
        self.0
            .iter_mut()
            .find(|(k, _)| key.matches(k))
            .map(|(_, v)| v)
    }
    pub fn get_utf16(&self, key: &[u16]) -> Option<&Value> {
        self.0
            .iter()
            .find(|(k, _)| k.as_utf16() == key)
            .map(|(_, v)| v)
    }
    pub fn get_utf16_mut(&mut self, key: &[u16]) -> Option<&mut Value> {
        self.0
            .iter_mut()
            .find(|(k, _)| k.as_utf16() == key)
            .map(|(_, v)| v)
    }
    pub fn contains_key<K: ObjectKey + ?Sized>(&self, key: &K) -> bool {
        self.get(key).is_some()
    }
    pub fn insert(&mut self, key: JsString, value: Value) -> Option<Value> {
        if let Some((_, old)) = self.0.iter_mut().find(|(k, _)| k == &key) {
            return Some(std::mem::replace(old, value));
        }
        self.0.push((key, value));
        None
    }
    pub fn remove<K: ObjectKey + ?Sized>(&mut self, key: &K) -> Option<Value> {
        let index = self.0.iter().position(|(k, _)| key.matches(k))?;
        Some(self.0.remove(index).1)
    }
    pub fn remove_utf16(&mut self, key: &[u16]) -> Option<Value> {
        let index = self.0.iter().position(|(k, _)| k.as_utf16() == key)?;
        Some(self.0.remove(index).1)
    }
    pub fn entry(&mut self, key: impl Into<JsString>) -> Entry<'_> {
        Entry {
            object: self,
            key: key.into(),
        }
    }
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = (&JsString, &Value)> + ExactSizeIterator {
        self.0.iter().map(|(k, v)| (k, v))
    }
    pub fn iter_mut(
        &mut self,
    ) -> impl DoubleEndedIterator<Item = (&JsString, &mut Value)> + ExactSizeIterator {
        self.0.iter_mut().map(|(k, v)| (&*k, v))
    }
    pub fn keys(&self) -> impl DoubleEndedIterator<Item = &JsString> + ExactSizeIterator {
        self.0.iter().map(|(k, _)| k)
    }
    pub fn values(&self) -> impl DoubleEndedIterator<Item = &Value> + ExactSizeIterator {
        self.0.iter().map(|(_, v)| v)
    }
    pub fn values_mut(
        &mut self,
    ) -> impl DoubleEndedIterator<Item = &mut Value> + ExactSizeIterator {
        self.0.iter_mut().map(|(_, v)| v)
    }
}
impl Clone for Object {
    fn clone(&self) -> Self {
        Self(self.0.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
    }
}
pub struct Entry<'a> {
    object: &'a mut Object,
    key: JsString,
}
impl<'a> Entry<'a> {
    pub fn or_insert(self, default: Value) -> &'a mut Value {
        self.or_insert_with(|| default)
    }
    pub fn or_insert_with(self, default: impl FnOnce() -> Value) -> &'a mut Value {
        let index = if let Some(index) = self.object.0.iter().position(|(k, _)| k == &self.key) {
            index
        } else {
            let index = self.object.len();
            self.object.0.push((self.key, default()));
            index
        };
        &mut self.object.0[index].1
    }
    pub fn or_default(self) -> &'a mut Value {
        self.or_insert(Value::Null)
    }
}
impl<K: Into<JsString>> FromIterator<(K, Value)> for Object {
    fn from_iter<T: IntoIterator<Item = (K, Value)>>(iter: T) -> Self {
        let mut object = Self::new();
        let mut positions: std::collections::HashMap<JsString, usize> =
            std::collections::HashMap::new();
        for (key, value) in iter {
            let key = key.into();
            if let Some(&position) = positions.get(&key) {
                object.0[position].1 = value;
            } else {
                positions.insert(key.clone(), object.len());
                object.0.push((key, value));
            }
        }
        object
    }
}
impl<K: Into<JsString>> Extend<(K, Value)> for Object {
    fn extend<T: IntoIterator<Item = (K, Value)>>(&mut self, iter: T) {
        for (key, value) in iter {
            self.insert(key.into(), value);
        }
    }
}
impl IntoIterator for Object {
    type Item = (JsString, Value);
    type IntoIter = std::vec::IntoIter<Self::Item>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}
fn pair_ref(pair: &(JsString, Value)) -> (&JsString, &Value) {
    (&pair.0, &pair.1)
}
fn pair_mut(pair: &mut (JsString, Value)) -> (&JsString, &mut Value) {
    (&pair.0, &mut pair.1)
}
impl<'a> IntoIterator for &'a Object {
    type Item = (&'a JsString, &'a Value);
    type IntoIter = std::iter::Map<
        std::slice::Iter<'a, (JsString, Value)>,
        fn(&'a (JsString, Value)) -> Self::Item,
    >;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter().map(pair_ref)
    }
}
impl<'a> IntoIterator for &'a mut Object {
    type Item = (&'a JsString, &'a mut Value);
    type IntoIter = std::iter::Map<
        std::slice::IterMut<'a, (JsString, Value)>,
        fn(&'a mut (JsString, Value)) -> Self::Item,
    >;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter_mut().map(pair_mut)
    }
}
