import { convertFileSrc } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";

import type { SequenceAudio } from "../../../types";
import { clamp } from "../shared";

type WaveformLevel = { samplesPerPeak: number; mins: Float32Array; maxes: Float32Array };
type WaveformAudio = { durationSeconds: number; sampleRate: number; levels: WaveformLevel[] };
export type WaveformState = { key: string | null; audio: WaveformAudio | null };

const WAVEFORM_CACHE_LIMIT = 4;
const waveformCache = new Map<string, { request: Promise<WaveformAudio | null>; lastUsed: number }>();
let waveformCacheAccess = 1;

export function useSequenceWaveform(audio: SequenceAudio | null): WaveformState {
  const key = audio?.exists === true ? audio.resolvedPath : null;
  const [state, setState] = useState<WaveformState>({ key, audio: null });

  useEffect(() => {
    if (key === null) return;
    let cancelled = false;
    let cached = waveformCache.get(key);
    if (cached === undefined) {
      cached = { request: decodeWaveformPeaks(key), lastUsed: waveformCacheAccess++ };
      waveformCache.set(key, cached);
      evictWaveformCache();
    } else {
      cached.lastUsed = waveformCacheAccess++;
    }
    void cached.request.then((waveform) => {
      if (!cancelled) setState({ key, audio: waveform });
    });
    return () => {
      cancelled = true;
    };
  }, [key]);

  return state.key === key ? state : { key, audio: null };
}

function evictWaveformCache() {
  while (waveformCache.size > WAVEFORM_CACHE_LIMIT) {
    let oldest: [string, number] | null = null;
    for (const [key, entry] of waveformCache) {
      if (oldest === null || entry.lastUsed < oldest[1]) oldest = [key, entry.lastUsed];
    }
    if (oldest === null) return;
    waveformCache.delete(oldest[0]);
  }
}

async function decodeWaveformPeaks(path: string): Promise<WaveformAudio | null> {
  try {
    const response = await fetch(convertFileSrc(path));
    if (!response.ok) return null;
    const context = new AudioContext();
    try {
      const buffer = await context.decodeAudioData(await response.arrayBuffer());
      return buildWaveformAudio(buffer);
    } finally {
      await context.close();
    }
  } catch {
    return null;
  }
}

function buildWaveformAudio(buffer: AudioBuffer): WaveformAudio {
  const samplesPerPeak = clamp(Math.round(buffer.sampleRate * 0.02), 512, 4096);
  const channels = Array.from({ length: buffer.numberOfChannels }, (_, index) => buffer.getChannelData(index));
  const bucketCount = Math.max(1, Math.ceil(buffer.length / samplesPerPeak));
  const mins = new Float32Array(bucketCount);
  const maxes = new Float32Array(bucketCount);
  for (let bucket = 0; bucket < bucketCount; bucket += 1) {
    const start = bucket * samplesPerPeak;
    const end = Math.min(buffer.length, start + samplesPerPeak);
    for (const channel of channels) {
      for (let index = start; index < end; index += 1) {
        const sample = channel[index] ?? 0;
        mins[bucket] = Math.min(mins[bucket] ?? 0, sample);
        maxes[bucket] = Math.max(maxes[bucket] ?? 0, sample);
      }
    }
  }
  const levels: WaveformLevel[] = [{ samplesPerPeak, mins, maxes }];
  while ((levels[levels.length - 1]?.mins.length ?? 0) > 1) {
    const previous = levels[levels.length - 1];
    if (previous === undefined) break;
    levels.push(coarsenWaveformLevel(previous));
  }
  return { durationSeconds: buffer.duration, sampleRate: buffer.sampleRate, levels };
}

function coarsenWaveformLevel(level: WaveformLevel): WaveformLevel {
  const bucketCount = Math.ceil(level.mins.length / 2);
  const mins = new Float32Array(bucketCount);
  const maxes = new Float32Array(bucketCount);
  for (let bucket = 0; bucket < bucketCount; bucket += 1) {
    const left = bucket * 2;
    const right = left + 1;
    mins[bucket] = Math.min(level.mins[left] ?? 0, level.mins[right] ?? level.mins[left] ?? 0);
    maxes[bucket] = Math.max(level.maxes[left] ?? 0, level.maxes[right] ?? level.maxes[left] ?? 0);
  }
  return { samplesPerPeak: level.samplesPerPeak * 2, mins, maxes };
}

export function drawWaveformStrip(
  ctx: CanvasRenderingContext2D,
  audio: WaveformAudio | null,
  left: number,
  top: number,
  width: number,
  height: number,
  durationSeconds: number,
  pxPerSecond: number,
  scrollXSeconds: number,
  colors: { grid: string; accent: string }
) {
  ctx.save();
  ctx.beginPath();
  ctx.rect(left, top, width, height);
  ctx.clip();
  ctx.strokeStyle = colors.grid;
  ctx.beginPath();
  ctx.moveTo(left, top + height / 2 + 0.5);
  ctx.lineTo(left + width, top + height / 2 + 0.5);
  ctx.stroke();
  if (audio !== null && audio.durationSeconds > 0 && audio.levels.length > 0) {
    const samplesPerPixel = audio.sampleRate / pxPerSecond;
    const level = audio.levels.find((item) => item.samplesPerPeak >= samplesPerPixel) ?? audio.levels[audio.levels.length - 1];
    if (level !== undefined) drawWaveformLevel(ctx, level, audio, left, width, height, top, durationSeconds, pxPerSecond, scrollXSeconds, colors.accent);
  }
  ctx.restore();
}

function drawWaveformLevel(ctx: CanvasRenderingContext2D, level: WaveformLevel, audio: WaveformAudio, left: number, width: number, height: number, top: number, durationSeconds: number, pxPerSecond: number, scrollXSeconds: number, color: string) {
  const xPerPeak = (level.samplesPerPeak / audio.sampleRate) * pxPerSecond;
  const clipEnd = Math.min(durationSeconds, audio.durationSeconds);
  const first = Math.max(0, Math.floor((Math.max(0, scrollXSeconds) * audio.sampleRate) / level.samplesPerPeak));
  const last = Math.min(level.mins.length - 1, Math.ceil((Math.min(clipEnd, scrollXSeconds + width / pxPerSecond) * audio.sampleRate) / level.samplesPerPeak));
  const centerY = top + height / 2;
  const amplitude = Math.max(1, height / 2 - 4);
  ctx.fillStyle = color;
  for (let index = first; index <= last; index += 1) {
    const seconds = (index * level.samplesPerPeak) / audio.sampleRate;
    const x = left + (seconds - scrollXSeconds) * pxPerSecond;
    if (seconds > clipEnd || x > left + width) break;
    if (x + xPerPeak < left) continue;
    const y1 = centerY - (level.maxes[index] ?? 0) * amplitude;
    const y2 = centerY - (level.mins[index] ?? 0) * amplitude;
    ctx.fillRect(x, y1, Math.max(1, xPerPeak), Math.max(1, y2 - y1));
  }
}
