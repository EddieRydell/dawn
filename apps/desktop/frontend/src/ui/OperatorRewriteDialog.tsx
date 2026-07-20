import * as AlertDialog from "@radix-ui/react-alert-dialog";
import { useEffect, useMemo, useState } from "react";

import { commands } from "../api";
import { useAppStore } from "../store";
import { THEME_COLORS } from "../theme";
import type {
  OperatorDefinitionCandidate,
  OperatorDefinitionKey,
  OperatorRewriteResolution,
  PendingOperatorRewrite,
  SequenceEffectParamValue
} from "../types";

export function OperatorRewriteDialog() {
  const pending = useAppStore((store) => store.snapshot?.pendingOperatorRewrite ?? null);
  return pending === null ? null : <OperatorRewriteForm key={pending.token} pending={pending} />;
}

function OperatorRewriteForm({ pending }: { pending: PendingOperatorRewrite }) {
  const [definitions, setDefinitions] = useState<Record<string, string>>(() =>
    Object.fromEntries(pending.definitions.map((item) => [definitionKey(item.definition), item.exactReplacement ?? ""]))
  );
  const [params, setParams] = useState<Record<string, string>>({});
  const [ports, setPorts] = useState<Record<string, string>>({});
  const [usageDefinitions, setUsageDefinitions] = useState<Record<string, string>>({});
  const [usageParams, setUsageParams] = useState<Record<string, string>>({});
  const [usagePorts, setUsagePorts] = useState<Record<string, string>>({});
  const [values, setValues] = useState<Record<string, string>>({});
  const [connections, setConnections] = useState<Record<string, string>>({});
  const [validationErrors, setValidationErrors] = useState<string[]>([]);
  const [valid, setValid] = useState(false);
  const [applying, setApplying] = useState(false);

  const resolution = useMemo(
    () => buildResolution(pending, definitions, params, ports, usageDefinitions, usageParams, usagePorts, values, connections),
    [pending, definitions, params, ports, usageDefinitions, usageParams, usagePorts, values, connections]
  );

  useEffect(() => {
    let current = true;
    void commands.validateOperatorRewrite(pending.token, resolution).then((result) => {
      if (!current) return;
      setValid(result.valid);
      setValidationErrors(result.errors);
    }).catch((error: unknown) => {
      if (!current) return;
      setValid(false);
      setValidationErrors([errorMessage(error)]);
    });
    return () => { current = false; };
  }, [pending, resolution]);

  const token = pending.token;

  async function cancel() {
    const snapshot = await commands.cancelOperatorRewrite(token);
    useAppStore.getState().setSnapshot(snapshot);
  }

  async function apply() {
    if (!valid) return;
    setApplying(true);
    try {
      const snapshot = await commands.applyOperatorRewrite(token, resolution);
      useAppStore.getState().setSnapshot(snapshot);
    } finally {
      setApplying(false);
    }
  }

  return (
    <AlertDialog.Root open>
      <AlertDialog.Portal>
        <AlertDialog.Overlay className="dialog-overlay" />
        <AlertDialog.Content className="dialog-content operator-rewrite-dialog">
          <AlertDialog.Title>Reconcile operator rewrite</AlertDialog.Title>
          <AlertDialog.Description>
            {pending.path} changes operator schemas used by the current project. Choose each migration before applying it.
          </AlertDialog.Description>
          <div className="operator-rewrite-scroll">
            {pending.definitions.map((definition) => {
              const key = definitionKey(definition.definition);
              const selectedName = definitions[key] ?? "";
              const selected = definition.candidates.find((candidate) => candidate.name === selectedName) ?? null;
              return (
                <details key={key} open>
                  <summary title={`${definition.definition.moduleId}:${definition.definition.document}`}>
                    {definition.oldName} · {definition.definition.document} · {definition.usageCount} usage{definition.usageCount === 1 ? "" : "s"}
                  </summary>
                  <label>
                    <span>Replacement definition</span>
                    <select value={selectedName} onChange={(event) => { setDefinitions({ ...definitions, [key]: event.target.value }); }}>
                      <option value="">Delete affected nodes</option>
                      {definition.candidates.map((candidate) => <option key={candidate.name} value={candidate.name}>{candidate.name}</option>)}
                    </select>
                  </label>
                  {selected !== null && definition.removedOrChangedParams.map((oldName) => (
                    <label key={`param-${oldName}`}>
                      <span>Parameter {oldName}</span>
                      <select value={params[mapKey(definition.definition, oldName)] ?? ""} onChange={(event) => { setParams({ ...params, [mapKey(definition.definition, oldName)]: event.target.value }); }}>
                        <option value="">Detach automation / discard value</option>
                        {selected.params.map((param) => <option key={param.name} value={param.name}>{param.name} ({param.valueType})</option>)}
                      </select>
                    </label>
                  ))}
                  {selected !== null && definition.removedPorts.map((oldName) => (
                    <label key={`port-${oldName}`}>
                      <span>Input port {oldName}</span>
                      <select value={ports[mapKey(definition.definition, oldName)] ?? ""} onChange={(event) => { setPorts({ ...ports, [mapKey(definition.definition, oldName)]: event.target.value }); }}>
                        <option value="">Disconnect</option>
                        {selected.inputPorts.map((port) => <option key={port} value={port}>{port}</option>)}
                      </select>
                    </label>
                  ))}
                  {selected !== null && definition.usages.map((usage) => {
                    const definitionOverride = usageDefinitions[usageKey(usage, "definition")];
                    const usageCandidate = definitionOverride === undefined || definitionOverride === "__global"
                      ? selected
                      : definition.candidates.find((candidate) => candidate.name === definitionOverride) ?? null;
                    const effectiveParams = usageMappings(definition, usage, params, usageParams);
                    const effectivePorts = usageMappings(definition, usage, ports, usagePorts);
                    return (
                    <details key={`${usage.sequencePath}-${usage.sequenceName}-${usage.nodeId}`}>
                      <summary>{usage.sequenceName} · node {usage.nodeId}</summary>
                      <label>
                        <span>Definition override</span>
                        <select value={definitionOverride ?? "__global"} onChange={(event) => { setUsageDefinitions({ ...usageDefinitions, [usageKey(usage, "definition")]: event.target.value }); }}>
                          <option value="__global">Use global ({selected.name})</option>
                          <option value="">Delete this node</option>
                          {definition.candidates.map((candidate) => <option key={candidate.name} value={candidate.name}>{candidate.name}</option>)}
                        </select>
                      </label>
                      {usageCandidate !== null && definition.removedOrChangedParams.map((oldName) => (
                        <label key={`usage-param-${oldName}`}>
                          <span>{oldName} override</span>
                          <select value={usageParams[usageKey(usage, oldName)] ?? "__global"} onChange={(event) => { setUsageParams({ ...usageParams, [usageKey(usage, oldName)]: event.target.value }); }}>
                            <option value="__global">Use global mapping</option>
                            <option value="">Detach / discard</option>
                            {usageCandidate.params.map((param) => <option key={param.name} value={param.name}>{param.name}</option>)}
                          </select>
                        </label>
                      ))}
                      {usageCandidate !== null && definition.removedPorts.map((oldName) => (
                        <label key={`usage-port-${oldName}`}>
                          <span>{oldName} port override</span>
                          <select value={usagePorts[usageKey(usage, oldName)] ?? "__global"} onChange={(event) => { setUsagePorts({ ...usagePorts, [usageKey(usage, oldName)]: event.target.value }); }}>
                            <option value="__global">Use global mapping</option>
                            <option value="">Disconnect</option>
                            {usageCandidate.inputPorts.map((port) => <option key={port} value={port}>{port}</option>)}
                          </select>
                        </label>
                      ))}
                      {usageCandidate !== null && requiredParams(definition, usageCandidate, effectiveParams).map((param) => {
                        const key = usageKey(usage, param.name);
                        return (
                          <label key={`required-${param.name}`}>
                            <span>Required {param.name} ({param.valueType})</span>
                            <input value={values[key] ?? defaultValueText(param.valueType)} onChange={(event) => { setValues({ ...values, [key]: event.target.value }); }} />
                          </label>
                        );
                      })}
                      {usageCandidate !== null && requiredPorts(definition, usageCandidate, effectivePorts).map((port) => {
                        const key = usageKey(usage, port);
                        return (
                          <label key={`connection-${port}`}>
                            <span>Connect {port}</span>
                            <select value={connections[key] ?? ""} onChange={(event) => { setConnections({ ...connections, [key]: event.target.value }); }}>
                              <option value="">Choose upstream source</option>
                              {usage.upstreamSources.map((source) => <option key={`${source.nodeId}-${source.port}`} value={`${source.nodeId}|${source.port}`}>{source.label}</option>)}
                            </select>
                          </label>
                        );
                      })}
                    </details>
                    );
                  })}
                </details>
              );
            })}
          </div>
          {validationErrors.length > 0 && <div className="new-project-error">{validationErrors.join(" ")}</div>}
          <div className="dialog-actions">
            <button type="button" disabled={applying} onClick={() => void cancel()}>Cancel</button>
            <button type="button" disabled={!valid || applying} onClick={() => void apply()}>{applying ? "Applying…" : "Apply"}</button>
          </div>
        </AlertDialog.Content>
      </AlertDialog.Portal>
    </AlertDialog.Root>
  );
}

function buildResolution(
  pending: PendingOperatorRewrite,
  definitions: Record<string, string>,
  params: Record<string, string>,
  ports: Record<string, string>,
  usageDefinitions: Record<string, string>,
  usageParams: Record<string, string>,
  usagePorts: Record<string, string>,
  values: Record<string, string>,
  connections: Record<string, string>
): OperatorRewriteResolution {
  const requiredValues: OperatorRewriteResolution["requiredValues"] = [];
  const requiredConnections: OperatorRewriteResolution["requiredConnections"] = [];
  for (const definition of pending.definitions) {
    const selected = definition.candidates.find((candidate) => candidate.name === definitions[definitionKey(definition.definition)]);
    if (selected === undefined) continue;
    for (const usage of definition.usages) {
      const definitionOverride = usageDefinitions[usageKey(usage, "definition")];
      const usageCandidate = definitionOverride === undefined || definitionOverride === "__global"
        ? selected
        : definition.candidates.find((candidate) => candidate.name === definitionOverride);
      if (usageCandidate === undefined) continue;
      const effectiveParams = usageMappings(definition, usage, params, usageParams);
      const effectivePorts = usageMappings(definition, usage, ports, usagePorts);
      for (const param of requiredParams(definition, usageCandidate, effectiveParams)) {
        requiredValues.push({
          sequencePath: usage.sequencePath,
          sequenceName: usage.sequenceName,
          nodeId: usage.nodeId,
          name: param.name,
          value: paramValue(param.valueType, values[usageKey(usage, param.name)] ?? defaultValueText(param.valueType))
        });
      }
      for (const port of requiredPorts(definition, usageCandidate, effectivePorts)) {
        const [fromNode = "", fromPort = ""] = (connections[usageKey(usage, port)] ?? "").split("|");
        if (fromNode !== "") requiredConnections.push({
          sequencePath: usage.sequencePath,
          sequenceName: usage.sequenceName,
          nodeId: usage.nodeId,
          inputPort: port,
          fromNode,
          fromPort
        });
      }
    }
  }
  return {
    definitions: pending.definitions.map((definition) => ({
      definition: definition.definition,
      replacementName: emptyToNull(definitions[definitionKey(definition.definition)])
    })),
    usageDefinitions: pending.definitions.flatMap((definition) => definition.usages.flatMap((usage) => {
      const replacementName = usageDefinitions[usageKey(usage, "definition")];
      return replacementName === undefined || replacementName === "__global" ? [] : [{
        sequencePath: usage.sequencePath,
        sequenceName: usage.sequenceName,
        nodeId: usage.nodeId,
        replacementName: emptyToNull(replacementName)
      }];
    })),
    parameters: pending.definitions.flatMap((definition) => definition.removedOrChangedParams.map((oldName) => ({
      definition: definition.definition,
      oldName,
      newName: emptyToNull(params[mapKey(definition.definition, oldName)])
    }))),
    usageParameters: pending.definitions.flatMap((definition) => definition.usages.flatMap((usage) => definition.removedOrChangedParams.flatMap((oldName) => {
      const newName = usageParams[usageKey(usage, oldName)];
      return newName === undefined || newName === "__global" ? [] : [{
        sequencePath: usage.sequencePath,
        sequenceName: usage.sequenceName,
        nodeId: usage.nodeId,
        oldName,
        newName: emptyToNull(newName)
      }];
    }))),
    ports: pending.definitions.flatMap((definition) => definition.removedPorts.map((oldName) => ({
      definition: definition.definition,
      oldName,
      newName: emptyToNull(ports[mapKey(definition.definition, oldName)])
    }))),
    usagePorts: pending.definitions.flatMap((definition) => definition.usages.flatMap((usage) => definition.removedPorts.flatMap((oldName) => {
      const newName = usagePorts[usageKey(usage, oldName)];
      return newName === undefined || newName === "__global" ? [] : [{
        sequencePath: usage.sequencePath,
        sequenceName: usage.sequenceName,
        nodeId: usage.nodeId,
        oldName,
        newName: emptyToNull(newName)
      }];
    }))),
    requiredValues,
    requiredConnections
  };
}

function requiredParams(
  definition: PendingOperatorRewrite["definitions"][number],
  candidate: OperatorDefinitionCandidate,
  mappings: Record<string, string>
) {
  const mapped = new Set(definition.removedOrChangedParams.map((name) => mappings[mapKey(definition.definition, name)]).filter(Boolean));
  const explicitlyNew = new Set(definition.newRequiredParams.map((param) => param.name));
  return candidate.params.filter((param) => param.required && !mapped.has(param.name) && (definition.exactReplacement === null || explicitlyNew.has(param.name)));
}

function usageMappings(
  definition: PendingOperatorRewrite["definitions"][number],
  usage: { sequencePath: string; sequenceName: string; nodeId: string },
  globalMappings: Record<string, string>,
  overrides: Record<string, string>
): Record<string, string> {
  return Object.fromEntries([
    ...definition.removedOrChangedParams,
    ...definition.removedPorts
  ].map((name) => {
    const override = overrides[usageKey(usage, name)];
    return [mapKey(definition.definition, name), override === undefined || override === "__global"
      ? globalMappings[mapKey(definition.definition, name)] ?? ""
      : override];
  }));
}

function requiredPorts(
  definition: PendingOperatorRewrite["definitions"][number],
  candidate: OperatorDefinitionCandidate,
  mappings: Record<string, string>
) {
  const mapped = new Set(definition.removedPorts.map((name) => mappings[mapKey(definition.definition, name)]).filter(Boolean));
  const explicitlyNew = new Set(definition.newPorts);
  return candidate.inputPorts.filter((port) => !mapped.has(port) && (definition.exactReplacement === null || explicitlyNew.has(port)));
}

function paramValue(valueType: string, text: string): SequenceEffectParamValue {
  if (valueType === "Int") return { type: "int", value: Number(text) };
  if (valueType === "Float") return { type: "float", value: Number(text) };
  if (valueType === "Bool") return { type: "bool", value: text === "true" };
  if (valueType === "Color") return { type: "color", value: text };
  if (valueType.startsWith("Enum")) return { type: "enum", value: text };
  if (valueType === "Curve") return { type: "curve", points: [] };
  if (valueType === "Gradient") return { type: "gradient", stops: [] };
  if (valueType === "Marks") return { type: "marks", key: text };
  if (valueType.startsWith("Array(Int")) return { type: "intArray", values: [] };
  if (valueType.startsWith("Array(Float")) return { type: "floatArray", values: [] };
  if (valueType.startsWith("Array(Bool")) return { type: "boolArray", values: [] };
  if (valueType.startsWith("Array(Color")) return { type: "colorArray", values: [] };
  if (valueType.startsWith("Array(Curve")) return { type: "curveArray", values: [] };
  return { type: "gradientArray", values: [] };
}

function defaultValueText(valueType: string): string {
  if (valueType === "Bool") return "false";
  if (valueType === "Color") return THEME_COLORS.defaultProjectColor;
  return "0";
}

function definitionKey(definition: OperatorDefinitionKey) {
  return `${definition.moduleId}\u0000${definition.document}\u0000${definition.name}`;
}

function mapKey(definition: OperatorDefinitionKey, name: string) {
  return `${definitionKey(definition)}\u0000${name}`;
}
function usageKey(usage: { sequencePath: string; sequenceName: string; nodeId: string }, name: string) {
  return `${usage.sequencePath}\u0000${usage.sequenceName}\u0000${usage.nodeId}\u0000${name}`;
}
function errorMessage(error: unknown) { return error instanceof Error ? error.message : String(error); }
function emptyToNull(value: string | undefined) { return value === undefined || value === "" ? null : value; }
