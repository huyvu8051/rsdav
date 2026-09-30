pub fn has_fields(fields: &[(&str, bool)]) -> bool {
    fields.iter().all(|(_, present)| *present)
}

pub fn find_missing_field_names(fields: &[(&str, bool)]) -> String {
    fields
        .iter()
        .filter(|(_, present)| !present)
        .map(|(name, _)| *name)
        .collect::<Vec<_>>()
        .join(",")
}
