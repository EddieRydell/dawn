import { invoke } from "@tauri-apps/api/core";

const FRAME_HEADER_LEN = 64;
const FRAME_OFFSET_PAYLOAD_BYTES = 12;
const FRAME_OFFSET_LATEST_SEQ = 16;
const FRAME_OFFSET_LATEST_SLOT = 20;
const FRAME_OFFSET_CURRENT_TIME = 24;
const FRAME_OFFSET_PLAYING = 32;
const FRAME_OFFSET_BACKEND_MS = 36;

export type PreviewTransportMode = "webview2_shared" | "unsupported";

export type SharedPreviewFrame = {
  frame: Uint8Array;
  currentTimeMs: number;
  playing: boolean;
  backendMs: number;
};

interface SharedBufferReceivedEvent extends Event {
  additionalData?: { kind?: string };
  getBuffer(): ArrayBuffer;
}

interface WebViewHost {
  addEventListener(type: "sharedbufferreceived", listener: (event: SharedBufferReceivedEvent) => void): void;
  removeEventListener(type: "sharedbufferreceived", listener: (event: SharedBufferReceivedEvent) => void): void;
  releaseBuffer(buffer: ArrayBuffer): void;
}

declare global {
  interface Window {
    chrome?: {
      webview?: WebViewHost;
    };
  }
}

let eventListenerInstalled = false;
let frameBuffer: ArrayBuffer | null = null;
let frameBytes: Uint8Array | null = null;
let frameView: DataView | null = null;
let rafPollHandle = 0;
let lastSeq = 0;
const listeners = new Set<(frame: SharedPreviewFrame) => void>();

function chromeWebview(): WebViewHost {
  const host = window.chrome?.webview;
  if (!host) {
    throw new Error("WebView2 shared preview transport is unavailable");
  }
  return host;
}

function releaseBuffer(buffer: ArrayBuffer | null): void {
  if (!buffer) return;
  try {
    chromeWebview().releaseBuffer(buffer);
  } catch {
    // Window teardown can race buffer release.
  }
}

function handleSharedBufferReceived(event: SharedBufferReceivedEvent): void {
  if (event.additionalData?.kind !== "frame") return;
  releaseBuffer(frameBuffer);
  frameBuffer = event.getBuffer();
  frameBytes = new Uint8Array(frameBuffer);
  frameView = new DataView(frameBuffer);
  lastSeq = 0;
}

function ensureEventListener(): void {
  if (eventListenerInstalled) return;
  chromeWebview().addEventListener("sharedbufferreceived", handleSharedBufferReceived);
  eventListenerInstalled = true;
}

async function waitForFrameBuffer(): Promise<void> {
  const deadline = performance.now() + 5000;
  while (performance.now() < deadline) {
    if (frameView && frameBytes) return;
    await new Promise((resolve) => window.setTimeout(resolve, 10));
  }
  throw new Error("Timed out waiting for shared preview buffer");
}

function publishFrame(message: SharedPreviewFrame): void {
  for (const listener of listeners) {
    listener(message);
  }
}

function startPolling(): void {
  if (rafPollHandle !== 0) return;
  const tick = () => {
    rafPollHandle = requestAnimationFrame(tick);
    if (!frameView || !frameBytes) return;

    const seqBefore = frameView.getUint32(FRAME_OFFSET_LATEST_SEQ, true);
    if (seqBefore === 0 || seqBefore === lastSeq) return;

    const payloadBytes = frameView.getUint32(FRAME_OFFSET_PAYLOAD_BYTES, true);
    const slot = frameView.getUint32(FRAME_OFFSET_LATEST_SLOT, true);
    const currentTimeMs = frameView.getFloat64(FRAME_OFFSET_CURRENT_TIME, true);
    const playing = frameView.getUint8(FRAME_OFFSET_PLAYING) === 1;
    const backendMs = frameView.getFloat32(FRAME_OFFSET_BACKEND_MS, true);
    const slotOffset = FRAME_HEADER_LEN + slot * payloadBytes;
    const slotEnd = slotOffset + payloadBytes;
    if (slotEnd > frameBytes.byteLength) return;

    const frame = frameBytes.subarray(slotOffset, slotEnd);
    const seqAfter = frameView.getUint32(FRAME_OFFSET_LATEST_SEQ, true);
    if (seqAfter !== seqBefore) return;

    lastSeq = seqBefore;
    publishFrame({ frame, currentTimeMs, playing, backendMs });
  };
  rafPollHandle = requestAnimationFrame(tick);
}

function stopPolling(): void {
  if (rafPollHandle !== 0) {
    cancelAnimationFrame(rafPollHandle);
    rafPollHandle = 0;
  }
}

export async function getPreviewTransportMode(): Promise<PreviewTransportMode> {
  return invoke<PreviewTransportMode>("get_preview_transport_mode");
}

export async function initPreviewTransport(): Promise<void> {
  ensureEventListener();
  frameBuffer = null;
  frameBytes = null;
  frameView = null;
  lastSeq = 0;
  await invoke("init_preview_transport");
  await waitForFrameBuffer();
  startPolling();
}

export async function disposePreviewTransport(): Promise<void> {
  stopPolling();
  listeners.clear();
  if (eventListenerInstalled) {
    try {
      chromeWebview().removeEventListener("sharedbufferreceived", handleSharedBufferReceived);
    } catch {
      // Window teardown can remove the host first.
    }
    eventListenerInstalled = false;
  }
  try {
    await invoke("dispose_preview_transport");
  } catch {
    // Ignore dispose failures during shutdown.
  }
  releaseBuffer(frameBuffer);
  frameBuffer = null;
  frameBytes = null;
  frameView = null;
  lastSeq = 0;
}

export function subscribePreviewFrames(listener: (frame: SharedPreviewFrame) => void): () => void {
  listeners.add(listener);
  startPolling();
  return () => {
    listeners.delete(listener);
  };
}
