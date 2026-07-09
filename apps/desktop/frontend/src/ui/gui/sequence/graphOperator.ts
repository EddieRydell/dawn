import type {
  SequenceGraphOperator,
  SequenceGraphOperatorDefinition
} from "../../../types";

export function graphOperatorDefinition(
  catalog: SequenceGraphOperatorDefinition[],
  operator: SequenceGraphOperator
) {
  const definition = catalog.find((candidate) => candidate.operator === operator);
  if (definition === undefined) {
    throw new Error(`Missing graph operator catalog entry for ${operator}`);
  }
  return definition;
}
