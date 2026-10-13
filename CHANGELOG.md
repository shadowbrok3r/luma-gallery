# Changelog

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
