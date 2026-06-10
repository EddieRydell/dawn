export type ActiveGuiDocumentDto =
  | { type: "sequence"; document: SequenceEditorDocumentDto }
  | { type: "layout"; document: LayoutDocumentDto }
  | { type: "fixture"; document: FixtureDocumentDto }
  | { type: "blocked"; reason: string; diagnostics: ProjectDiagnosticDto[] };

export type AppSnapshotDto = {
  projectRoot: string | null;
  projectTreeVisible: boolean;
  projectEntries: WorkspaceEntryDto[];
  tabs: EditorBufferDto[];
  activeFile: string | null;
  activeBuffer: EditorBufferDto | null;
  activeDocumentDescriptor: DocumentDescriptorDto | null;
  activeGuiDocument: ActiveGuiDocumentDto | null;
  diagnostics: ProjectDiagnosticDto[];
  status: string;
  sequenceTransport: SequenceTransportSnapshotDto;
  liveOutput: LiveOutputSnapshotDto;
};

export type AudioPlaybackStatus = "none" | "missing" | "ready" | "playing" | "ended" | "error";
export type BufferExternalStateDto = "current" | "changedOnDisk" | "deletedOnDisk";
export type ColorCurvePointDto = { time: number; value: string };
export type DiagnosticSeverityDto = "error" | "warning";
export type DocumentViewIdDto = "text" | "layout" | "fixture" | "sequence";
export type EditorViewModeDto = "text" | "gui";
export type LayoutTargetKindDto = "group" | "fixture";
export type ObjectKindDto =
  | "project"
  | "display"
  | "controller"
  | "layout"
  | "fixture"
  | "patch"
  | "sequence"
  | "curve"
  | "effect";
export type SequenceCurveValueTypeDto = "float" | "color";
export type SequenceEffectParamKindDto =
  | "int"
  | "float"
  | "bool"
  | "color"
  | "enum"
  | "flags"
  | "floatCurve"
  | "colorCurve"
  | "intArray"
  | "floatArray"
  | "boolArray"
  | "colorArray"
  | "floatCurveArray"
  | "colorCurveArray"
  | "marks";
export type SequenceEffectScopeDto = "perFixture" | "wholeTarget";
export type SequenceEffectScriptKindDto = "sample" | "generator";
export type SequenceResizeEdgeDto = "left" | "right";
export type SequenceTransportState = "stopped" | "paused" | "playing" | "ended" | "error";
export type WorkspaceEntryKindDto = "directory" | "file";

export type DocumentDefaultObjectKeyDto = {
  view: DocumentViewIdDto;
  objectKey: string;
};

export type DocumentDescriptorDto = {
  path: string;
  objects: DocumentObjectDescriptorDto[];
  availableViews: DocumentViewIdDto[];
  defaultObjectKeys: DocumentDefaultObjectKeyDto[];
};

export type DocumentObjectDescriptorDto = {
  key: string;
  kind: ObjectKindDto;
};

export type EditorBufferDto = {
  path: string;
  name: string;
  text: string;
  dirty: boolean;
  externalState: BufferExternalStateDto;
  viewMode: EditorViewModeDto;
};

export type EffectScriptReferenceDto = {
  path: string;
  effectName: string;
};

export type FixtureDefinitionDto = {
  objectKey: string;
  name: string;
  colorModel: string;
  bulbDiameterMeters: number;
  geometry: GeometryDto;
  geometrySummary: string;
  renderPlan: GeometryRenderPlanDto;
};

export type FixtureDocumentDto = {
  path: string;
  selectedObjectKey: string | null;
  fixtures: FixtureDefinitionDto[];
};

export type FixtureGuiEditDto =
  | { type: "updateBulbDiameter"; objectKey: string; bulbDiameterMeters: number }
  | { type: "movePoint"; objectKey: string; pointIndex: number; point: Point3MetersDto };

export type FloatCurvePointDto = { time: number; value: number };

export type GeometryDto =
  | { type: "points"; points: Point3MetersDto[] }
  | { type: "lines"; points: Point3MetersDto[]; pixels: number }
  | {
      type: "arc";
      center: Point3MetersDto;
      radiusMeters: number;
      startDegrees: number;
      endDegrees: number;
      pixels: number;
    };

export type GeometryRenderBoundsDto = {
  minXMeters: number;
  minYMeters: number;
  maxXMeters: number;
  maxYMeters: number;
};

export type GeometryRenderGuideDto =
  | { type: "line"; from: GeometryRenderPointDto; to: GeometryRenderPointDto }
  | {
      type: "arc";
      start: GeometryRenderPointDto;
      end: GeometryRenderPointDto;
      radiusXMeters: number;
      radiusYMeters: number;
      rotation: number;
      largeArc: boolean;
      sweepPositive: boolean;
    };

export type GeometryRenderPlanDto = {
  emitters: GeometryRenderPointDto[];
  guides: GeometryRenderGuideDto[];
  bounds: GeometryRenderBoundsDto;
  bulbRadiusMeters: number;
};

export type GeometryRenderPointDto = {
  xMeters: number;
  yMeters: number;
  zMeters: number;
};

export type LayoutDocumentDto = {
  path: string;
  objectKey: string;
  name: string;
  renderBounds: GeometryRenderBoundsDto;
  fixtures: LayoutFixturePlacementDto[];
};

export type LayoutFixturePlacementDto = {
  id: number;
  name: string;
  transform: TransformDto;
  resolvedFixture: ResolvedLayoutFixtureDto;
};

export type LayoutGuiEditDto = { type: "updatePlacementTransform"; id: number; transform: TransformDto };
export type LayoutTargetDto = { kind: LayoutTargetKindDto; name: string };

export type LiveOutputSnapshotDto = {
  enabled: boolean;
  status: string;
  activeUniverseCount: number;
  lastError: string | null;
};

export type Point3MetersDto = {
  xMeters: number;
  yMeters: number;
  zMeters: number;
};

export type ProjectDiagnosticDto = {
  path: string;
  range: TextRangeDto | null;
  severity: DiagnosticSeverityDto;
  code: string;
  message: string;
};

export type ResolvedLayoutFixtureDto = {
  name: string;
  colorModel: string;
  bulbDiameterMeters: number;
  geometrySummary: string;
  renderPlan: GeometryRenderPlanDto;
  sourcePath: string;
  objectKey: string | null;
};

export type Rotation3DegreesDto = {
  xDegrees: number;
  yDegrees: number;
  zDegrees: number;
};

export type Scale3Dto = {
  x: number;
  y: number;
  z: number;
};

export type SequenceAudioDto = {
  import: string;
  resolvedPath: string;
  fileName: string;
  exists: boolean;
};

export type SequenceCurveLibraryItemDto = {
  path: string;
  objectKey: string;
  displayName: string;
  valueType: SequenceCurveValueTypeDto;
  points: SequenceCurveLibraryPointsDto;
};

export type SequenceCurveLibraryPointsDto =
  | { type: "float"; points: FloatCurvePointDto[] }
  | { type: "color"; points: ColorCurvePointDto[] };

export type SequenceEditorDocumentDto = {
  path: string;
  objectKey: string;
  durationSeconds: number;
  frameRate: number;
  audio: SequenceAudioDto | null;
  markCollections: SequenceMarkCollectionDto[];
  lanes: SequenceLaneDto[];
  effectScripts: SequenceEffectScriptDto[];
  curveLibrary: SequenceCurveLibraryItemDto[];
  effects: SequenceEffectDto[];
  degraded: boolean;
};

export type SequenceEffectDto = {
  index: number;
  id: number;
  startSeconds: number;
  durationSeconds: number;
  target: LayoutTargetDto;
  targetLabel: string;
  scope: SequenceEffectScopeDto;
  script: string;
  scriptSource: EffectScriptReferenceDto | null;
  params: SequenceEffectParamDto[];
};

export type SequenceEffectParamCurveSourceDto =
  | { type: "inline" }
  | {
      type: "library";
      reference: string;
      path: string | null;
      objectKey: string | null;
      displayName: string | null;
    };

export type SequenceEffectParamDto = {
  name: string;
  kind: SequenceEffectParamKindDto;
  options: string[];
  editable: boolean;
  value: SequenceEffectParamValueDto;
  curveSource: SequenceEffectParamCurveSourceDto | null;
};

export type SequenceEffectParamValueDto =
  | { type: "int"; value: number }
  | { type: "float"; value: number }
  | { type: "bool"; value: boolean }
  | { type: "color"; value: string }
  | { type: "enum"; value: string }
  | { type: "flags"; value: string[] }
  | { type: "floatCurve"; points: FloatCurvePointDto[] }
  | { type: "colorCurve"; points: ColorCurvePointDto[] }
  | { type: "intArray"; values: number[] }
  | { type: "floatArray"; values: number[] }
  | { type: "boolArray"; values: boolean[] }
  | { type: "colorArray"; values: string[] }
  | { type: "floatCurveArray"; values: FloatCurvePointDto[][] }
  | { type: "colorCurveArray"; values: ColorCurvePointDto[][] }
  | { type: "marks"; key: string };

export type SequenceEffectScriptDto = {
  name: string;
  kind: SequenceEffectScriptKindDto;
  script: EffectScriptReferenceDto;
  import: string;
  params: SequenceEffectScriptParamDto[];
};

export type SequenceEffectScriptParamDto = {
  name: string;
  kind: SequenceEffectParamKindDto;
};

export type SequenceGuiEditDto =
  | { type: "setAudio"; import: string | null }
  | {
      type: "addEffect";
      script: EffectScriptReferenceDto;
      target: LayoutTargetDto;
      scope: SequenceEffectScopeDto;
      startSeconds: number;
      markCollectionKey: string | null;
    }
  | { type: "moveEffect"; id: number; startSeconds: number; target: LayoutTargetDto | null }
  | { type: "resizeEffect"; id: number; startSeconds: number; durationSeconds: number }
  | { type: "changeEffectScript"; id: number; script: EffectScriptReferenceDto }
  | { type: "deleteEffect"; id: number }
  | { type: "retargetEffect"; id: number; target: LayoutTargetDto }
  | { type: "setEffectScope"; id: number; scope: SequenceEffectScopeDto }
  | { type: "updateEffectParam"; id: number; name: string; value: SequenceEffectParamValueDto }
  | { type: "linkEffectCurveParam"; id: number; name: string; curvePath: string; objectKey: string }
  | { type: "unlinkEffectCurveParam"; id: number; name: string }
  | { type: "createMarkCollection"; key: string; name: string; color: string }
  | { type: "renameMarkCollection"; key: string; name: string }
  | { type: "deleteMarkCollection"; key: string }
  | { type: "setMarkCollectionColor"; key: string; color: string }
  | { type: "addMark"; collectionKey: string; timeSeconds: number }
  | { type: "moveMark"; collectionKey: string; index: number; timeSeconds: number }
  | { type: "deleteMark"; collectionKey: string; index: number };

export type SequenceKeyDto = { path: string; objectKey: string };
export type SequenceLaneDto = { target: LayoutTargetDto; label: string };
export type SequenceMarkCollectionDto = { key: string; name: string; color: string; marksSeconds: number[] };
export type SequenceMarkRefDto = { collectionKey: string; index: number };
export type SequencePasteAnchorDto = { laneIndex: number; timeSeconds: number };
export type SequenceSelectionDto =
  | { type: "effects"; ids: number[] }
  | { type: "marks"; marks: SequenceMarkRefDto[] };

export type SequenceSelectionEditDto =
  | { type: "copy"; selection: SequenceSelectionDto }
  | { type: "cut"; selection: SequenceSelectionDto }
  | { type: "delete"; selection: SequenceSelectionDto }
  | { type: "paste"; anchor: SequencePasteAnchorDto }
  | { type: "moveEffects"; ids: number[]; timeDeltaSeconds: number; laneDelta: number }
  | { type: "resizeEffects"; ids: number[]; edge: SequenceResizeEdgeDto; timeDeltaSeconds: number }
  | { type: "moveMarks"; marks: SequenceMarkRefDto[]; timeDeltaSeconds: number };

export type SequenceSelectionEditResultDto = {
  snapshot: AppSnapshotDto;
  selection: SequenceSelectionDto | null;
  copiedCount: number;
  skippedCount: number;
};

export type SequenceTransportSnapshotDto = {
  sourceLabel: string;
  sourceKey: SequenceKeyDto | null;
  renderGeneration: number;
  renderDirtyRevision: number;
  transportState: SequenceTransportState;
  renderUpdating: boolean;
  positionSeconds: number;
  homeSeconds: number;
  durationSeconds: number;
  audio: SequenceAudioDto | null;
  clockSource: string;
  audioPlaybackStatus: AudioPlaybackStatus;
  geometryIdentity: string;
  status: string;
};

export type TextPositionDto = { line: number; character: number };
export type TextRangeDto = { start: TextPositionDto; end: TextPositionDto };

export type TransformDto = {
  position: Point3MetersDto;
  rotation: Rotation3DegreesDto;
  scale: Scale3Dto;
};

export type WorkspaceEntryDto = {
  path: string;
  kind: WorkspaceEntryKindDto;
  name: string;
  parent: string;
};
