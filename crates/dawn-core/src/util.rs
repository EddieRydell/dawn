use serde::Serialize;

pub fn serde_variant_names<T>(variants: &[T]) -> Vec<String>
where
    T: Serialize,
{
    variants
        .iter()
        .filter_map(|variant| serde_json::to_value(variant).ok())
        .filter_map(|value| value.as_str().map(ToOwned::to_owned))
        .collect()
}
