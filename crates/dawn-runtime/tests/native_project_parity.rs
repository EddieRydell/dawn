use dawn_language::dsl::compile_effects;
use dawn_language::effect::{EffectDefinition, EffectDefinitionId, EffectRef};
use dawn_language::identity::{DocumentId, SourceIdentity};
use dawn_project_io::load_package;
use dawn_runtime::PreparedSequenceRenderer;

#[test]
fn native_effects_match_reference_dsl_in_real_project_frames() {
    let root = camino::Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter");
    let session = load_package(&root).unwrap().session;
    let mut reference_project = session.project.clone();
    let document = DocumentId::new(
        session.source.project_module_id(),
        "tests/native-effect-reference.effect.dawn".into(),
    );
    for compiled in compile_effects(include_str!(
        "../../dawn-language/tests/fixtures/native_effect_reference.effect.dawn"
    ))
    .unwrap()
    {
        let id = EffectDefinitionId(SourceIdentity::from_document(
            document.clone(),
            compiled.name().as_str().to_string(),
        ));
        reference_project
            .definitions
            .effects
            .insert(id.clone(), EffectDefinition::custom(id, compiled));
    }
    for sequence in reference_project.sequences.values_mut() {
        for effect in &mut sequence.effects {
            let EffectRef::Builtin(builtin) = effect.definition else {
                continue;
            };
            let name = match builtin {
                dawn_language::effect::BuiltinEffect::Pulse => "Pulse",
                dawn_language::effect::BuiltinEffect::Chase => "Chase",
                dawn_language::effect::BuiltinEffect::Spin => "Spin",
                dawn_language::effect::BuiltinEffect::MarkPulse => "MarkPulse",
                dawn_language::effect::BuiltinEffect::MarkChase => "MarkChase",
            };
            effect.definition = EffectRef::Custom(EffectDefinitionId(
                SourceIdentity::from_document(document.clone(), name.to_string()),
            ));
        }
    }
    let setup = &session.project.root.setup;
    let sequence = &session.project.root.sequences[0];
    let native = PreparedSequenceRenderer::prepare(&session.project, setup, sequence).unwrap();
    let reference = PreparedSequenceRenderer::prepare(&reference_project, setup, sequence).unwrap();
    for frame in [144, 2088, 5904, 9504, 11520, 19080, 7707] {
        assert_eq!(
            native.render_frame(frame).unwrap(),
            reference.render_frame(frame).unwrap(),
            "frame {frame}"
        );
    }
    let sequence = &session.project.root.sequences[1];
    let native = PreparedSequenceRenderer::prepare(&session.project, setup, sequence).unwrap();
    let reference = PreparedSequenceRenderer::prepare(&reference_project, setup, sequence).unwrap();
    assert_eq!(
        native.render_frame(3594).unwrap(),
        reference.render_frame(3594).unwrap()
    );
}
