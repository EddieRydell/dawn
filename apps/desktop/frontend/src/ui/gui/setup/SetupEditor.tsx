import { useState, type ReactNode } from "react";

import { commands } from "../../../api";
import { runGuiEditCommand } from "../../../store";
import type { SetupDocument } from "../../../types";

export function SetupEditor({ document }: { document: SetupDocument }) {
  return (
    <div className="setup-editor">
      <header>
        <div>
          <h2>Display Setup</h2>
          <span>{document.objectKey}</span>
        </div>
        <span>{document.elements.length} elements · {document.controllers.length} controllers</span>
      </header>
      <div className="setup-sections">
        <SetupSection title="Elements">
          <div className="setup-table">
            {document.elements.map((element) => (
              <div className="setup-row" key={element.id}>
                <span className="setup-id">{element.id}</span>
                <input
                  defaultValue={element.name}
                  aria-label={`Element ${element.id} name`}
                  onBlur={(event) => {
                    if (event.currentTarget.value !== element.name) {
                      void runGuiEditCommand(() => commands.applySetupGuiEdit({
                        type: "renameElement",
                        id: element.id,
                        name: event.currentTarget.value
                      }));
                    }
                  }}
                />
                <span>{element.kind}</span>
                {element.cellCount !== null ? (
                  <input
                    className="setup-number"
                    type="number"
                    min={1}
                    defaultValue={element.cellCount}
                    aria-label={`Element ${element.id} cells`}
                    onBlur={(event) => {
                      const cells = Number(event.currentTarget.value);
                      if (cells !== element.cellCount) {
                        void runGuiEditCommand(() => commands.applySetupGuiEdit({
                          type: "setElementCellCount",
                          id: element.id,
                          cells
                        }));
                      }
                    }}
                  />
                ) : <span>{element.children.length} children</span>}
                <span>{element.capability ?? element.profile ?? ""}</span>
              </div>
            ))}
          </div>
        </SetupSection>

        <SetupSection title="Fixture Profiles">
          {document.fixtureProfiles.length === 0 ? <Empty label="No fixture profiles" /> : document.fixtureProfiles.map((profile) => (
            <div className="setup-summary" key={profile.id}>
              <strong>{profile.name}</strong>
              <span>{profile.functionCount} functions · {profile.channelCount} channels · {profile.behaviorRuleCount} behavior rules</span>
            </div>
          ))}
        </SetupSection>

        <SetupSection title="Preview Links">
          {document.previewLinks.map((link) => <PreviewLink key={link.propId} link={link} document={document} />)}
        </SetupSection>

        <SetupSection title="Patching">
          <div className="patch-node-grid">
            {document.patchNodes.map((node) => (
              <div className={`patch-node-card ${node.kind}`} key={node.id}>
                <span>#{node.id} · {node.kind}</span>
                <strong>{node.label}</strong>
                <span>{node.width} values</span>
              </div>
            ))}
          </div>
          <div className="setup-edge-list">
            {document.patchEdges.map((edge) => (
              <button
                key={`${edge.fromNode}:${edge.fromPort}:${edge.toNode}:${edge.toPort}`}
                title="Remove patch edge"
                onClick={() => void runGuiEditCommand(() => commands.applySetupGuiEdit({
                  type: "disconnectPatch",
                  ...edge
                }))}
              >
                {edge.fromNode}:{edge.fromPort} → {edge.toNode}:{edge.toPort}
              </button>
            ))}
          </div>
        </SetupSection>

        <SetupSection title="Controllers">
          {document.controllers.map((controller) => (
            <div className="controller-card" key={controller.id}>
              <div className="setup-summary">
                <strong>{controller.id}</strong>
                <span>{controller.protocol} · {controller.mode} · bind {controller.bindAddress}{controller.destination !== null ? ` · ${controller.destination}` : ""}</span>
              </div>
              {controller.ports.map((port) => <ControllerPort key={port.id} controller={controller.id} port={port} />)}
            </div>
          ))}
        </SetupSection>
      </div>
    </div>
  );
}

function PreviewLink({ link, document }: { link: SetupDocument["previewLinks"][number]; document: SetupDocument }) {
  const [node, setNode] = useState(document.elements.find((element) => element.cellCount !== null)?.id ?? 0);
  const [startCell, setStartCell] = useState(0);
  return (
    <div className="preview-link-row">
      <div>
        <strong>{link.name}</strong>
        <span>{link.pointCount} points · {link.bindings.length} explicit bindings</span>
      </div>
      <select value={node} onChange={(event) => { setNode(Number(event.currentTarget.value)); }}>
        {document.elements.filter((element) => element.cellCount !== null).map((element) => (
          <option value={element.id} key={element.id}>{element.name}</option>
        ))}
      </select>
      <input className="setup-number" type="number" min={0} value={startCell} onChange={(event) => { setStartCell(Number(event.currentTarget.value)); }} />
      <button onClick={() => void runGuiEditCommand(() => commands.applySetupGuiEdit({
        type: "autoLinkPreview",
        propId: link.propId,
        node,
        startCell
      }))}>Auto-link</button>
    </div>
  );
}

function ControllerPort({ controller, port }: { controller: string; port: SetupDocument["controllers"][number]["ports"][number] }) {
  const [address, setAddress] = useState(port.address);
  const [slotCount, setSlotCount] = useState(port.slotCount);
  return (
    <div className="controller-port-row">
      <span>Port {port.id}</span>
      <label>Address <input type="number" min={0} value={address} onChange={(event) => { setAddress(Number(event.currentTarget.value)); }} /></label>
      <label>Slots <input type="number" min={1} max={512} value={slotCount} onChange={(event) => { setSlotCount(Number(event.currentTarget.value)); }} /></label>
      <button onClick={() => void runGuiEditCommand(() => commands.applySetupGuiEdit({
        type: "setControllerPort",
        controller,
        port: port.id,
        address,
        slotCount
      }))}>Apply</button>
    </div>
  );
}

function SetupSection({ title, children }: { title: string; children: ReactNode }) {
  return <section className="setup-section"><h3>{title}</h3>{children}</section>;
}

function Empty({ label }: { label: string }) { return <span className="setup-empty">{label}</span>; }
