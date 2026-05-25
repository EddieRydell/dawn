use crate::ui::theme;

pub fn assert_required_fonts_available() {
    let mut database = fontdb::Database::new();
    database.load_system_fonts();

    let available_families = database
        .faces()
        .flat_map(|face| face.families.iter())
        .map(|(family, _)| family.as_str())
        .collect::<Vec<_>>();

    let missing_fonts = [theme::APP_FONT, theme::MONO_FONT]
        .into_iter()
        .filter(|required| {
            !available_families
                .iter()
                .any(|available| available.eq_ignore_ascii_case(required))
        })
        .collect::<Vec<_>>();

    if missing_fonts.is_empty() {
        return;
    }

    let message = format!(
        "Required font family not installed: {}. Dawn does not use font fallbacks; install the missing font or update the theme font constant.",
        missing_fonts.join(", ")
    );
    eprintln!("{message}");
    panic!("{message}");
}
