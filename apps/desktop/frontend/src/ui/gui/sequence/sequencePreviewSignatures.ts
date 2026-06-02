import type { LayoutTargetDto, SequenceDocumentDto, SequenceEffectDto, SequenceMarkCollectionDto } from "../../../bindings";

export function targetsEqual(left: LayoutTargetDto, right: LayoutTargetDto) {
  return left.kind === right.kind && left.name === right.name;
}

export function sequencePreviewSignatures(document: SequenceDocumentDto) {
  return new Map<number, string>(
    document.effects.map((effect) => [
      effect.id,
      JSON.stringify({
        path: document.path,
        objectKey: document.objectKey,
        frameRate: document.frameRate,
        id: effect.id,
        durationSeconds: effect.durationSeconds,
        target: effect.target,
        scope: effect.scope,
        script: effect.script,
        params: effect.params,
        markCollections: relevantMarkCollections(effect, document.markCollections)
      })
    ])
  );
}

function relevantMarkCollections(effect: SequenceEffectDto, markCollections: SequenceMarkCollectionDto[]) {
  const keys = effect.params
    .flatMap((param) => (param.value.type === "marks" ? [param.value.key] : []));
  if (keys.length === 0) return [];
  return {
    effectStartSeconds: effect.startSeconds,
    collections: markCollections
      .filter((collection) => keys.includes(collection.key))
      .map((collection) => ({ key: collection.key, marksSeconds: collection.marksSeconds }))
  };
}
