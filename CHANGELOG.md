# Changelog

## v0.1.17 — 2026-10-16

Scrolling has momentum and rubber-bands at the ends, and buttons are a little roomier with
rounder corners (egui 0.37).

## v0.1.16 — 2026-10-13

Video editor: Speed plays a segment from 0.25x to 4x with the sound's pitch kept; captions over
it stretch with it. Adjust grades a segment with exposure, brightness, contrast, highlights,
shadows, whites, blacks, temperature, tint, vibrance and saturation, shown live on the video and
applied in exports through the same color table. Playback controls sit under the timeline, and
the tools are grouped under Video, Text, Overlay and Canvas tabs instead of one scrolling row.
Viewer: speed, loop and sound get their own row beside the resolution and frame rate, which
Export clip no longer covers; play, frame steps and previous/next are on the bottom row.
Settings > Playback controls places these controls on the right, left or center.
Fixed: Remove captions crashed the app when the captions were the top layer. Pausing a sped-up
segment now shows the frame at the reported time.

## v0.1.15 — 2026-10-13

Text styles: TikTok Sans is bundled and listed after Roboto. Its variable weight draws regular
and bold, its slant axis draws italics, and its optical size follows the text size. Fonts a
device also has are listed once.

## v0.1.14 — 2026-10-13

Video editor: add more videos to the timeline from the Video tool (at the cut nearest the
playhead) or the + tile after the last clip (at the end). Each clip keeps its own audio; other
sizes and rotations are letterboxed into the canvas in the preview and the export.
Playback: scrubbing, Play and Loop work again after a video reaches its end; looping no longer
reports "This recording could not play".
Captions from speech: the Captions tool transcribes the timeline's audio on the phone with
whisper.cpp (a 57 MB model downloads on first use) and adds a linked caption layer shown as
phrases, one word at a time, or phrases with the spoken word highlighted. Captions use the text
styles; moving or restyling one moves or restyles them all. Exports draw each overlay layer as one
FFmpeg input, with animations baked per frame, so hundreds of captions export quickly.
FFmpeg now includes the concat demuxer and WAV muxer. Media tools start with fork/exec; the
previous posix_spawn could hang a tool for its whole timeout when two started at once.

## v0.1.13 — 2026-10-12

Video editor: split, delete with ripple and segment fades; caption and photo overlay layers with
translucent timeline clips, overlap bands, visibility, locking and reordering; drag, snap, scale
and rotate on the video; fonts, sizes, colors, outlines, boxes, shadows, presets and in/out
animations; canvas crop, audio volume and fades, undo/redo, autosave and a debug panel. Exports
compose the edit with FFmpeg, which now includes the overlay, fade, concat and related filters.

## v0.1.12 — 2026-10-09

Update EguiMobile and the vendored Android IME bridge to 8098d9d. Leaving an
editing field preserves its composing word, including the Qwen prompt.
