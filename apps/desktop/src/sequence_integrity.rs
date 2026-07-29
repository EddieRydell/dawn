use dawn_language::sequence::SequenceId;
use dawn_project_io::ProjectSession;

use crate::gui::GuiMutationError;

pub(crate) fn validate_sequence_integrity(
    session: &ProjectSession,
    id: &SequenceId,
) -> Result<(), GuiMutationError> {
    let sequence = session
        .project
        .sequences
        .get(id)
        .ok_or_else(|| GuiMutationError::Invalid("Sequence was not found.".to_string()))?;
    dawn_language::validation::validate_sequence(&session.project, sequence)
        .map_err(|error| GuiMutationError::Invalid(error.message))
}
