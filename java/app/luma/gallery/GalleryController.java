package app.luma.gallery;

import android.app.Activity;
import android.content.*;
import android.graphics.*;
import android.media.*;
import android.net.Uri;
import android.os.*;
import android.provider.*;
import android.system.ErrnoException;
import android.system.Os;
import android.system.OsConstants;
import android.view.*;
import android.widget.*;
import org.json.*;
import org.videolan.libvlc.LibVLC;
import org.videolan.libvlc.Media;
import org.videolan.libvlc.MediaPlayer;
import org.videolan.libvlc.interfaces.IMedia;
import org.videolan.libvlc.interfaces.IVLCVout;
import java.io.*;
import java.lang.Process;
import java.util.*;
import java.util.concurrent.*;
import java.util.concurrent.atomic.*;

final class GalleryController {
    private final GalleryActivity activity;
    private final Handler main = new Handler(Looper.getMainLooper());
    private final MediaFiles files;
    private final ConcurrentLinkedQueue<JSONObject> events = new ConcurrentLinkedQueue<>();
    private final ExecutorService io = Executors.newSingleThreadExecutor();
    private final ExecutorService incoming = Executors.newSingleThreadExecutor();
    private final AtomicLong incomingGeneration = new AtomicLong();
    private final ThreadPoolExecutor thumbs = (ThreadPoolExecutor) Executors.newFixedThreadPool(1);
    private final ExecutorService previews = Executors.newSingleThreadExecutor();
    private final ExecutorService photos = Executors.newSingleThreadExecutor();
    private final ExecutorService exports = Executors.newSingleThreadExecutor();
    private final ExecutorService changes = Executors.newSingleThreadExecutor();
    private final MediaManager manager;
    private final PhotoEdits edits;
    private final QwenEdits qwen;
    private final AudioWaveform waveform;
    private final VideoFrames frames;
    private final Transcriber speech;
    private static final int REQUEST_CONSENT = 702;
    private volatile CompletableFuture<Boolean> consent;
    private volatile boolean managing, manageMedia, rescan;
    private volatile double manageProgress;
    private volatile String manageStatus = "";
    private final Set<String> thumbnailPending = ConcurrentHashMap.newKeySet();
    private final AtomicReference<Process> exportProcess = new AtomicReference<>();
    private final AtomicReference<Process> rawProcess = new AtomicReference<>();
    private final AtomicBoolean exportCancelled = new AtomicBoolean();
    private final AtomicLong previewGeneration = new AtomicLong();
    private final AtomicBoolean previewBusy = new AtomicBoolean();
    private volatile PreviewRequest previewRequest;
    private static final class PreviewRequest {
        final JSONObject item;
        final long session, time;
        final String label;
        PreviewRequest(JSONObject item, long session, long time, String label) {
            this.item = item; this.session = session; this.time = time; this.label = label;
        }
    }
    private volatile JSONObject selected;
    private volatile JSONObject playback = new JSONObject();
    private volatile boolean closed, scanning, exporting;
    private volatile double exportProgress;
    private volatile String exportStatus = "", exportUri = "";
    private volatile long generation;
    private FrameLayout root;
    private SurfaceView surface;
    private Surface gpuSurface;
    private int bufferWidth = 1920, bufferHeight = 1080;
    private PhotoView photo;
    private PopupWindow photoWindow;
    private LibVLC vlc;
    private MediaPlayer player;
    private MediaFiles.Source source;
    private int videoWidth, videoHeight, stageX, stageY, stageWidth, stageHeight;
    private boolean stageVisible = true, muted, loop;
    private long loopStart, loopEnd;
    private long[] rangeIn = new long[0], rangeOut = new long[0];
    private int[] rangeSource = new int[0];
    private String[] sourceUris = new String[0];
    /** Edit segment being shown, and the uri of the media in the player. */
    private int segment;
    private String loadedUri = "";
    private long clockTime = -1, clockAt, jumpTarget = -1, jumpAt, hold = -1;
    private boolean finished;
    private int pauses;
    private float speed = 1;
    private boolean software;
    private String photoStatus = "", playbackError = "";
    private PopupWindow loupe;
    private ImageView loupeImage;
    private TextView loupeTime;
    private AudioFocusRequest focus;
    private final AudioManager audio;
    private final BroadcastReceiver noisy;

    GalleryController(GalleryActivity activity) {
        this.activity = activity;
        files = new MediaFiles(activity);
        manager = new MediaManager(activity);
        edits = new PhotoEdits(activity, files, this::emit, this::scan);
        qwen = new QwenEdits(edits, this::emit);
        waveform = new AudioWaveform(files, this::emit);
        frames = new VideoFrames(activity, files, this::emit, this::scan);
        speech = new Transcriber(activity, files, this::emit);
        refreshAccess();
        root = activity.findViewById(android.R.id.content);
        audio = (AudioManager) activity.getSystemService(Context.AUDIO_SERVICE);
        focus = new AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN)
            .setAudioAttributes(new AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_MEDIA).setContentType(AudioAttributes.CONTENT_TYPE_MOVIE).build())
            .setOnAudioFocusChangeListener(change -> { if (change < 0) pause(); }).build();
        noisy = new BroadcastReceiver() { @Override public void onReceive(Context c, Intent i) { pause(); } };
        if (Build.VERSION.SDK_INT >= 33) activity.registerReceiver(noisy, new IntentFilter(AudioManager.ACTION_AUDIO_BECOMING_NOISY), Context.RECEIVER_NOT_EXPORTED);
        else activity.registerReceiver(noisy, new IntentFilter(AudioManager.ACTION_AUDIO_BECOMING_NOISY));
        scan();
        main.post(tick);
    }

    private final Runnable tick = new Runnable() {
        @Override public void run() {
            if (closed) return;
            JSONObject state = new JSONObject();
            try {
                boolean playing = player != null && player.isPlaying() && hold < 0;
                long position = player == null ? 0 : Math.max(0, player.getTime());
                long duration = player == null ? 0 : Math.max(0, player.getLength());
                if (finished && !playing) position = Math.max(position, span(false) - 40);
                if (playing) activity.getWindow().addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
                else activity.getWindow().clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
                state.put("playing", playing).put("position", position).put("duration", duration)
                    .put("width", videoWidth).put("height", videoHeight).put("muted", muted)
                    .put("rate", speed).put("error", playbackError).put("photo_status", photoStatus)
                    .put("zoom", photo == null ? 1 : photo.zoomValue());
                if (selected != null) state.put("uri", selected.optString("uri"));
                if (rangeIn.length > 0) state.put("segment", segment);
            } catch (Exception ignored) { }
            playback = state;
            main.postDelayed(this, player != null && player.isPlaying() ? 33 : 100);
        }
    };

    /** Keeps playback inside the edit's ranges, the loop or the file, extrapolating LibVLC's coarse clock between updates. */
    private final Runnable rangeWatch = new Runnable() {
        @Override public void run() {
            if (closed || player == null) return;
            boolean playing = player.isPlaying() && hold < 0;
            if (playing) {
                long time = player.getTime(), now = SystemClock.uptimeMillis();
                boolean settling = now - jumpAt < 600
                    && (time < jumpTarget - 100 || time > jumpTarget + 100 + (long)((now - jumpAt) * speed));
                if (time != clockTime) { clockTime = time; clockAt = now; }
                long at = clockTime + (long)(Math.min(400, now - clockAt) * speed);
                long start = span(true), end = span(false);
                if (settling) { }
                else if (rangeIn.length > 0) follow(at, now);
                else if (end <= 0) { }
                else if (at >= end - 25 && loop && loopEnd > loopStart) jump(start, now);
                else if (at >= end - 25) park(end - 40);
                else if (at < start - 60) jump(start, now);
            } else clockTime = -1;
            main.postDelayed(this, playing ? 15 : 60);
        }
    };

    /** Moves to the next edit segment as playback crosses the current one's end. */
    private void follow(long at, long now) {
        int i = Math.min(segment, rangeIn.length - 1);
        if (at < rangeIn[i] - 60) { jump(rangeIn[i], now); return; }
        if (at < rangeOut[i] - 25) return;
        int next = i + 1;
        if (next >= rangeIn.length) { park(Math.max(rangeIn[i], rangeOut[i] - 40)); return; }
        segment = next;
        if (!uriOf(next).equals(loadedUri)) load(uriOf(next), rangeIn[next], true);
        else if (Math.abs(rangeIn[next] - rangeOut[i]) > 40) jump(rangeIn[next], now);
    }

    private void park(long time) {
        pausePlayer();
        player.setTime(time, false);
        clockTime = -1; finished = true;
    }

    private String uriOf(int index) {
        int source = index < rangeSource.length ? rangeSource[index] : 0;
        if (source < sourceUris.length) return sourceUris[source];
        return selected == null ? "" : selected.optString("uri");
    }

    private void jump(long target, long now) {
        player.setTime(target, false);
        jumpTarget = target; jumpAt = now; clockTime = target; clockAt = now; finished = false;
    }

    /** Start or end of what plays: the edit's ranges, the loop selection or the whole file. */
    private long span(boolean start) {
        if (rangeIn.length > 0) return start ? rangeIn[0] : rangeOut[rangeOut.length - 1];
        long length = player == null ? 0 : player.getLength();
        if (!loop || loopEnd <= loopStart) return start ? 0 : length;
        return start ? loopStart : length > 0 ? Math.min(loopEnd, length) : loopEnd;
    }

    void emit(JSONObject event) { if (!closed) events.add(event); }
    void attachVideoSurface(Surface surface,int width,int height) { main.post(() -> { gpuSurface=surface; bufferWidth=width; bufferHeight=height; layoutStage(); }); }
    void error(String message) { android.util.Log.e("Luma", String.valueOf(message)); try { emit(new JSONObject().put("type", "error").put("message", message)); } catch (Exception ignored) { } }

    String poll() {
        try {
            JSONArray batch = new JSONArray();
            JSONObject event;
            for (int i = 0; i < 32 && (event = events.poll()) != null; i++) batch.put(event);
            return new JSONObject().put("events", batch).put("playback", playback).put("scanning", scanning)
                .put("access", files.access()).put("exporting", exporting).put("progress", exportProgress)
                .put("export_status", exportStatus).put("export_uri", exportUri).put("sdk", Build.VERSION.SDK_INT)
                .put("manage_media", manageMedia).put("managing", managing).put("manage_status", manageStatus)
                .put("manage_progress", manageProgress).put("photo_saving", edits.busy).put("photo_edit_status", edits.status)
                .put("qwen_busy", qwen.busy).put("qwen_status", qwen.status)
                .put("frame_saving", frames.busy).put("speech_busy", speech.busy).put("speech_status", speech.status)
                .put("speech_progress", speech.progress).put("speech_model", speech.modelReady)
                .put("locale", Locale.getDefault().getLanguage()).toString();
        } catch (Exception e) { return "{}"; }
    }

    void command(String json) {
        try {
            JSONObject data = new JSONObject(json);
            String op = data.getString("op");
            if ("thumb".equals(op)) { thumbnail(data); return; }
            main.post(() -> {
                if (closed) return;
                try {
                    switch (op) {
                        case "scan": scan(); break;
                        case "permission": activity.requestMediaPermissions(); break;
                        case "folder":
                            Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT_TREE);
                            intent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION | Intent.FLAG_GRANT_WRITE_URI_PERMISSION
                                | Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION | Intent.FLAG_GRANT_PREFIX_URI_PERMISSION);
                            activity.startActivityForResult(intent, 701); break;
                        case "open": open(data.getJSONObject("item")); break;
                        case "close": closeMedia(); break;
                        case "rect":
                            stageX = data.getInt("x"); stageY = data.getInt("y"); stageWidth = data.getInt("w"); stageHeight = data.getInt("h");
                            stageVisible = data.optBoolean("visible", true); layoutStage(); break;
                        case "play": togglePlay(data.optBoolean("playing", true)); break;
                        case "seek": seek(Math.max(0, data.getLong("time")), data.optBoolean("fast", false), data.optInt("segment", -1)); break;
                        case "segments": ranges(data.optJSONArray("ranges"), data.optJSONArray("sources")); break;
                        case "probe": probe(data.getJSONObject("item")); break;
                        case "studio_export": studioExport(data); break;
                        case "transcribe": speech.run(data); break;
                        case "cancel_transcribe": speech.cancel(); break;
                        case "speech_model_delete": speech.deleteModel(); break;
                        case "mute": muted = data.getBoolean("muted"); if (player != null && hold < 0) player.setVolume(muted ? 0 : 100); break;
                        case "rate": speed = (float)data.getDouble("rate"); if (player != null) player.setRate(speed); break;
                        case "loop": loop = data.getBoolean("enabled"); loopStart = data.optLong("start"); loopEnd = data.optLong("end"); break;
                        case "preview": showPreview(data); break;
                        case "preview_end": endPreview(); break;
                        case "zoom": if (photo != null) photo.zoom((float)data.optDouble("scale", 0)); break;
                        case "keyframe": locateKeyframe(data); break;
                        case "export": export(data, false); break;
                        case "capture_frame":
                            if (selected != null && "video".equals(selected.optString("kind"))
                                    && selected.optString("uri").equals(data.optString("uri"))) {
                                // While playing, sample the clock on the player thread at the tap.
                                // A paused/pending seek uses the exact position chosen in the UI.
                                long at = player != null && player.isPlaying() ? player.getTime() : data.optLong("time");
                                togglePlay(false);
                                frames.save(new JSONObject(selected.toString()), Math.max(0, at));
                            }
                            break;
                        case "photo_prepare": edits.prepare(data); break;
                        case "photo_save": edits.save(data); break;
                        case "photo_external": edits.external(data); break;
                        case "qwen_config": qwen.config(data); break;
                        case "qwen_edit": qwen.run(data,false); break;
                        case "qwen_resume": qwen.run(data,true); break;
                        case "qwen_stop": qwen.stop(); break;
                        case "waveform": waveform.read(data.getJSONObject("item")); break;
                        case "proxy": export(data, true); break;
                        case "cancel_export": exportCancelled.set(true); Process process = exportProcess.get(); if (process != null) process.destroyForcibly(); break;
                        case "share": share(data.optString("uri")); break;
                        case "share_many": shareMany(data.getJSONArray("uris")); break;
                        case "manage": manage(data); break;
                        case "cancel_manage": manager.cancelled.set(true); break;
                        case "manage_access": requestManageMedia(); break;
                        case "software": software = data.getBoolean("enabled"); if (selected != null && "video".equals(selected.optString("kind"))) open(selected); break;
                        case "diagnostics": diagnostics(); break;
                        default: error("Unknown command: " + op);
                    }
                } catch (Exception e) { error(e.getMessage()); }
            });
        } catch (Exception e) { error("Command: " + e.getMessage()); }
    }

    /** Scans the library; a request during a scan runs once more afterwards. */
    /** Cold launches and warm onNewIntent deliveries use the same queued UI event. */
    void receive(Intent intent) {
        if (intent == null || closed) return;
        String action = intent.getAction();
        if (!Intent.ACTION_VIEW.equals(action) && !Intent.ACTION_SEND.equals(action)
                && !Intent.ACTION_SEND_MULTIPLE.equals(action)) return;
        long request = incomingGeneration.incrementAndGet();
        Set<Uri> uris = new LinkedHashSet<>();
        try {
            if (Intent.ACTION_VIEW.equals(action)) {
                if (intent.getData() != null) uris.add(intent.getData());
            } else {
                if (Intent.ACTION_SEND_MULTIPLE.equals(action)) {
                    ArrayList<?> streams = intent.getParcelableArrayListExtra(Intent.EXTRA_STREAM);
                    if (streams != null) for (Object stream : streams) if (stream instanceof Uri) uris.add((Uri)stream);
                } else {
                    Object stream = intent.getParcelableExtra(Intent.EXTRA_STREAM);
                    if (stream instanceof Uri) uris.add((Uri)stream);
                }
                // ClipData is also used by senders that omit EXTRA_STREAM.
                ClipData clip = intent.getClipData();
                if (clip != null) for (int i = 0; i < clip.getItemCount(); i++) {
                    Uri uri = clip.getItemAt(i).getUri();
                    if (uri != null) uris.add(uri);
                }
            }
        } catch (RuntimeException e) {
            error("Couldn't read this share. Try sharing the photo or video again.");
            return;
        }
        String mime = intent.getType();
        incoming.execute(() -> {
            JSONArray items = new JSONArray();
            int skipped = 0;
            for (Uri uri : uris) {
                if (closed || incomingGeneration.get() != request) return;
                try { items.put(files.sharedItem(uri, mime)); }
                catch (Exception e) { skipped++; }
            }
            final int failed = skipped;
            // Checking on the main thread prevents an older slow provider replacing a newer share.
            main.post(() -> {
                if (closed || incomingGeneration.get() != request) return;
                if (items.length() == 0) {
                    error("Couldn't open this share. Share a readable photo or video from the source app again.");
                    return;
                }
                try { emit(new JSONObject().put("type", "incoming").put("items", items)); }
                catch (JSONException ignored) { }
                if (failed > 0) error("Opened " + items.length() + "; " + failed + " unavailable or unsupported files skipped.");
            });
        });
    }

    void scan() {
        synchronized (this) {
            if (closed) return;
            if (scanning) { rescan = true; return; }
            scanning = true;
        }
        io.execute(() -> {
            while (true) {
                try { emit(new JSONObject().put("type", "library").put("items", files.library()).put("trash", files.trash())); }
                catch (Exception e) { error("Library: " + e.getMessage()); }
                synchronized (GalleryController.this) {
                    if (!rescan || closed) { scanning = false; return; }
                    rescan = false;
                }
            }
        });
    }

    void activityResult(int request, int result, Intent data) {
        if (request == 703) { scan(); return; }
        if (request == REQUEST_CONSENT) {
            CompletableFuture<Boolean> answer = consent;
            if (answer != null) answer.complete(result == Activity.RESULT_OK);
            return;
        }
        if (request != 701 || result != Activity.RESULT_OK || data == null || data.getData() == null) return;
        try {
            activity.getContentResolver().takePersistableUriPermission(data.getData(),
                data.getFlags() & (Intent.FLAG_GRANT_READ_URI_PERMISSION | Intent.FLAG_GRANT_WRITE_URI_PERMISSION));
            scan();
        } catch (Exception e) { error("Folder access: " + e.getMessage()); }
    }

    void refreshAccess() { manageMedia = Build.VERSION.SDK_INT >= 31 && MediaStore.canManageMedia(activity); }

    private void manage(JSONObject request) {
        if (managing) { error("Wait for the current change to finish"); return; }
        managing = true; manageStatus = "Waiting for confirmation"; manageProgress = 0;
        changes.execute(() -> {
            JSONObject event = new JSONObject();
            try {
                event.put("type", "managed").put("action", request.optString("action")).put("target", request.optString("target"));
                MediaManager.Result result = manager.run(request, this::requestConsent, (status, fraction) -> { manageStatus = status; manageProgress = fraction; });
                event.put("done", result.done).put("failed", result.failed).put("cancelled", result.cancelled)
                    .put("error", result.error).put("uris", result.uris).put("renamed", result.renamed);
            } catch (Exception e) {
                android.util.Log.e("Luma", "Change failed", e);
                try { event.put("failed", 1).put("error", String.valueOf(e.getMessage())); } catch (Exception ignored) { }
            } finally {
                managing = false; manageStatus = "";
                emit(event);
                scan();
            }
        });
    }

    /** Shows a system confirmation and blocks the change thread until it is answered. */
    private boolean requestConsent(IntentSender request) throws Exception {
        CompletableFuture<Boolean> answer = new CompletableFuture<>();
        consent = answer;
        main.post(() -> {
            try { if (closed) answer.complete(false); else activity.startIntentSenderForResult(request, REQUEST_CONSENT, null, 0, 0, 0); }
            catch (Exception e) { answer.completeExceptionally(e); }
        });
        try { return answer.get(); }
        finally { consent = null; }
    }

    private void requestManageMedia() {
        if (Build.VERSION.SDK_INT < 31) { error("Requires Android 12 or newer"); return; }
        activity.startActivity(new Intent(Settings.ACTION_REQUEST_MANAGE_MEDIA, Uri.parse("package:" + activity.getPackageName())));
    }

    void thumbnail(JSONObject request) {
        String key = request.optString("key");
        if (thumbnailPending.size() >= 64 || !thumbnailPending.add(key)) return;
        thumbs.execute(() -> {
            try {
                File file = files.thumbnail(request.getJSONObject("item"), request.optLong("time"), request.optInt("width", 384));
                emit(new JSONObject().put("type", "thumb").put("key", key).put("path", file.getAbsolutePath()));
            } catch (Exception e) {
                android.util.Log.w("Luma", "Thumbnail: " + e.getMessage());
                try { emit(new JSONObject().put("type", "thumb_failed").put("key", key)); } catch (Exception ignored) { }
            } finally { thumbnailPending.remove(key); }
        });
    }

    private void open(JSONObject item) throws Exception {
        closeMedia();
        selected = item;
        final long token = generation;
        if ("video".equals(item.optString("kind"))) {
            videoWidth = item.optInt("width") > 0 ? item.optInt("width") : 1920;
            videoHeight = item.optInt("height") > 0 ? item.optInt("height") : 1080;
            source = files.source(item.getString("uri"));
            if (vlc == null) vlc = new LibVLC(activity, new ArrayList<>(Arrays.asList("--no-video-title-show", "--file-caching=250", "--audio-time-stretch", "--no-sub-autodetect-file")));
            if (gpuSurface == null) {
                surface = new SurfaceView(activity);
                surface.setZOrderOnTop(true);
                surface.setContentDescription("Video preview");
                surface.setOnClickListener(v -> togglePlay(player != null && !player.isPlaying()));
                root.addView(surface, new FrameLayout.LayoutParams(1,1));
            }
            player = new MediaPlayer(vlc);
            IVLCVout output = player.getVLCVout();
            if (gpuSurface != null) output.setVideoSurface(gpuSurface,null);
            else output.setVideoView(surface);
            output.attachViews((vout,w,h,vw,vh,sn,sd) -> {
                if (token != generation) return;
                videoWidth = vw > 0 ? vw : w; videoHeight = vh > 0 ? vh : h;
                videoTrackSize();
                layoutStage();
            });
            player.setEventListener(event -> {
                if (token != generation) return;
                if(event.type==MediaPlayer.Event.Playing || event.type==MediaPlayer.Event.Vout) {
                    videoTrackSize(); layoutStage();
                }
                if (event.type == MediaPlayer.Event.Playing) {
                    playbackError = ""; pauses = 0;
                    if (hold >= 0) { pausePlayer(); player.setTime(hold, false); player.setVolume(muted ? 0 : 100); hold = -1; }
                }
                // Unrequested pauses come from play-and-pause at the end of the input.
                if (event.type == MediaPlayer.Event.Paused) {
                    if (pauses > 0) pauses--;
                    else if (segment + 1 < rangeIn.length) {
                        segment++;
                        if (!uriOf(segment).equals(loadedUri)) load(uriOf(segment), rangeIn[segment], true);
                        else { jump(rangeIn[segment], SystemClock.uptimeMillis()); player.play(); }
                    }
                    else if (rangeIn.length == 0 && loop && loopEnd > loopStart) { jump(loopStart, SystemClock.uptimeMillis()); player.play(); }
                    else finished = true;
                }
                if (event.type == MediaPlayer.Event.EncounteredError) {
                    if (hold >= 0) { player.setVolume(muted ? 0 : 100); hold = -1; }
                    playbackError = "This recording could not play. Try a playback proxy.";
                    error(playbackError);
                }
                if (event.type == MediaPlayer.Event.EndReached && rangeIn.length == 0 && loop && loopEnd > loopStart) reopen(loopStart, true);
            });
            attach(source); loadedUri = item.getString("uri");
            player.setVolume(muted ? 0 : 100); player.setRate(speed);
            layoutStage(); togglePlay(true);
            main.removeCallbacks(rangeWatch); main.post(rangeWatch);
            io.execute(() -> {
                try {
                    JSONObject metadata = files.probe(item);
                    if (token == generation) emit(new JSONObject().put("type", "metadata").put("uri", item.getString("uri")).put("probe", metadata));
                } catch (Exception e) {
                    android.util.Log.w("Luma", "Optional video details unavailable", e);
                    if (token == generation) try {
                        emit(new JSONObject().put("type", "metadata").put("uri", item.optString("uri"))
                            .put("probe", new JSONObject().put("error", "Additional video details unavailable")));
                    } catch (Exception ignored) { }
                }
            });
        } else {
            photo = new PhotoView(activity, this);
            photoWindow = new PopupWindow(photo,1,1,false);
            photoWindow.setClippingEnabled(true);
            layoutStage();
            if ("raw".equals(item.optString("kind"))) {
                photoStatus = "Developing full-resolution RAW";
                photos.execute(() -> {
                    try {
                        File preview = files.thumbnail(item,0,1600);
                        main.post(() -> { if (token == generation && photo != null) { photoStatus = "RAW preview"; photo.load(preview.getAbsolutePath(), false); } });
                    } catch (Exception ignored) { }
                    try {
                        if (token != generation) return;
                        File full = files.developRaw(item, rawProcess);
                        main.post(() -> { if (token == generation && photo != null) { photoStatus = "Full-resolution RAW"; photo.load(full.getAbsolutePath(), true); } });
                    } catch (Exception e) { if (token == generation) { photoStatus = "RAW development failed"; error(e.getMessage()); } }
                });
            } else {
                source = files.source(item.getString("uri"));
                photoStatus = "Full resolution";
                photo.load(source.fd.getFileDescriptor(),Uri.parse(item.getString("uri")));
            }
        }
    }

    /** Gives the player `src` as its media. */
    private void attach(MediaFiles.Source src) {
        Media media = new Media(vlc, src.fd.getFileDescriptor());
        media.setHWDecoderEnabled(!software, false);
        media.addOption(":file-caching=250");
        media.addOption(":play-and-pause");
        player.setMedia(media); media.release();
    }

    private void layoutStage() {
        if (photoWindow != null) {
            if (stageVisible && stageWidth > 0 && stageHeight > 0) {
                if (photoWindow.isShowing()) photoWindow.update(stageX,stageY,stageWidth,stageHeight);
                else { photoWindow.setWidth(stageWidth); photoWindow.setHeight(stageHeight); photoWindow.showAtLocation(root,Gravity.TOP|Gravity.LEFT,stageX,stageY); }
            } else photoWindow.dismiss();
            return;
        }
        View view = surface != null ? surface : photo;
        if (view == null) {
            if(player!=null) player.getVLCVout().setWindowSize(bufferWidth,bufferHeight);
            return;
        }
        view.setVisibility(stageVisible && stageWidth > 0 && stageHeight > 0 ? View.VISIBLE : View.INVISIBLE);
        int width = Math.max(1,stageWidth), height = Math.max(1,stageHeight), x = stageX, y = stageY;
        if (surface != null && videoWidth > 0 && videoHeight > 0) {
            float fit = Math.min(width/(float)videoWidth, height/(float)videoHeight);
            int actualWidth = Math.max(1,Math.round(videoWidth*fit)), actualHeight = Math.max(1,Math.round(videoHeight*fit));
            x += (width-actualWidth)/2; y += (height-actualHeight)/2; width=actualWidth; height=actualHeight;
        }
        int[] origin = new int[2]; root.getLocationInWindow(origin);
        FrameLayout.LayoutParams params = new FrameLayout.LayoutParams(width,height);
        params.leftMargin = x-origin[0]; params.topMargin = y-origin[1];
        view.setLayoutParams(params);
        if (player != null) player.getVLCVout().setWindowSize(width,height);
    }

    /** Hardware Surface output may not report a new-video-layout callback. */
    private void videoTrackSize() {
        org.videolan.libvlc.interfaces.IMedia.VideoTrack track=player==null?null:player.getCurrentVideoTrack();
        if(track==null || track.width<=0 || track.height<=0)return;
        videoWidth=track.width;videoHeight=track.height;
        if(track.orientation>=org.videolan.libvlc.interfaces.IMedia.VideoTrack.Orientation.LeftTop) {
            int swap=videoWidth;videoWidth=videoHeight;videoHeight=swap;
        }
        android.util.Log.d("LumaVideo","Track "+track.width+"x"+track.height+" orientation="+track.orientation+" surface="+videoWidth+"x"+videoHeight);
    }

    private void togglePlay(boolean play) {
        if (player == null) return;
        if (!play) {
            // An opening input is paused from its Playing event.
            if (hold >= 0) return;
            if (player.getPlayerState() == IMedia.State.Opening) { hold = Math.max(0, position()); player.setVolume(0); }
            else pausePlayer();
            return;
        }
        if (audio.requestAudioFocus(focus) != AudioManager.AUDIOFOCUS_REQUEST_GRANTED) return;
        boolean edit = rangeIn.length > 0;
        if (edit && finished) seek(rangeIn[0], false, 0);
        long start = span(true), end = span(false), at = position();
        if (ended()) { reopen(edit ? Math.max(0, at) : at >= 0 && at < end - 400 ? at : start, true); return; }
        if (hold >= 0) { hold = -1; player.setVolume(muted ? 0 : 100); }
        if (!edit && (finished || (end > 0 && at < start - 60))) jump(start, SystemClock.uptimeMillis());
        player.play();
    }

    /** Seeks the media in the player, or edit segment `index` of the edit, loading its video when needed. */
    private void seek(long time, boolean fast, int index) {
        if (player == null) return;
        if (index >= 0 && index < rangeIn.length) {
            segment = index;
            if (!uriOf(index).equals(loadedUri)) { load(uriOf(index), time, player.isPlaying() && hold < 0); return; }
        }
        long length = player.getLength();
        if (length > 0) time = Math.min(time, Math.max(0, length - 40));
        if (ended()) { reopen(time, false); return; }
        if (hold >= 0) hold = time;
        player.setTime(time, fast);
        jumpTarget = time; jumpAt = SystemClock.uptimeMillis(); clockTime = -1; finished = false;
    }

    private void pausePlayer() {
        if (player.isPlaying()) pauses++;
        player.pause();
    }

    /** The latest seek target while LibVLC's coarse clock catches up, else the player's time. */
    private long position() {
        return SystemClock.uptimeMillis() - jumpAt < 600 ? jumpTarget : player.getTime();
    }

    /** Input states in which LibVLC ignores play() and seeks. */
    private boolean ended() {
        int state = player.getPlayerState();
        return state == IMedia.State.Ended || state == IMedia.State.Stopped || state == IMedia.State.Error;
    }

    /** Restarts the input at `time` from a rewound descriptor. */
    private void reopen(long time, boolean play) {
        player.stop();
        if (source != null) try { Os.lseek(source.fd.getFileDescriptor(), 0, OsConstants.SEEK_SET); }
        catch (ErrnoException e) { android.util.Log.w("Luma", "Rewind: " + e.getMessage()); }
        start(time, play);
    }

    /** Puts another video of the edit in the player at `time`. */
    private void load(String uri, long time, boolean play) {
        MediaFiles.Source next;
        try { next = files.source(uri); }
        catch (Exception e) { error("Could not open a video in the edit: " + e.getMessage()); return; }
        MediaFiles.Source previous = source;
        attach(next);
        source = next; loadedUri = uri;
        if (previous != null) try { previous.close(); } catch (Exception ignored) { }
        start(time, play);
    }

    /** Starts the player's input at `time`, held paused until its Playing event unless `play`. */
    private void start(long time, boolean play) {
        hold = play ? -1 : time; pauses = 0;
        player.setVolume(play && !muted ? 100 : 0);
        player.play();
        player.setTime(time, false);
        jumpTarget = time; jumpAt = SystemClock.uptimeMillis(); clockTime = -1; finished = false;
    }

    private void probe(JSONObject item) {
        io.execute(() -> {
            JSONObject details;
            try { details = files.probe(item); }
            catch (Exception e) { android.util.Log.w("Luma", "Probe: " + e.getMessage()); details = new JSONObject(); }
            try { emit(new JSONObject().put("type", "probe").put("uri", item.optString("uri")).put("probe", details)); }
            catch (JSONException ignored) { }
        });
    }

    void pause() { main.post(() -> { togglePlay(false); endPreview(); }); }

    void photoReady(PhotoView origin,int width,int height) {
        if (photo != origin || selected == null) return;
        try { emit(new JSONObject().put("type", "photo").put("uri", selected.optString("uri")).put("width", width).put("height", height).put("status", photoStatus)); }
        catch (Exception ignored) { }
    }

    void photoPreviewReady(PhotoView origin,int width,int height) {
        if(photo!=origin)return;
        photoStatus="Still preview"; photoReady(origin,width,height);
    }

    void navigatePhoto(PhotoView origin,int delta) {
        if (photo != origin || selected == null || "video".equals(selected.optString("kind"))) return;
        try { emit(new JSONObject().put("type", "navigate").put("uri", selected.optString("uri")).put("delta", delta)); }
        catch (Exception ignored) { }
    }

    private void closeMedia() {
        waveform.cancel();
        generation++;
        endPreview();
        Process raw = rawProcess.get(); if (raw != null) raw.destroyForcibly();
        if (player != null) { player.setEventListener(null); player.stop(); player.getVLCVout().detachViews(); player.release(); player = null; }
        if (surface != null) { root.removeView(surface); surface = null; }
        if (photoWindow != null) { photoWindow.dismiss(); photoWindow=null; }
        if (photo != null) { photo.close(); photo = null; }
        if (source != null) try { source.close(); } catch (Exception ignored) { }
        source = null; selected = null; loop = false; playbackError = ""; photoStatus = "";
        rangeIn = new long[0]; rangeOut = new long[0]; rangeSource = new int[0]; sourceUris = new String[0];
        segment = 0; loadedUri = ""; hold = -1; main.removeCallbacks(rangeWatch);
        audio.abandonAudioFocusRequest(focus);
        activity.getWindow().clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
    }

    private void showPreview(JSONObject request) throws Exception {
        if (selected == null || !"video".equals(selected.optString("kind"))
                || !selected.optString("uri").equals(request.optString("uri"))) return;
        if (loupe == null) {
            LinearLayout content = new LinearLayout(activity); content.setOrientation(LinearLayout.VERTICAL);
            float density = activity.getResources().getDisplayMetrics().density;
            android.graphics.drawable.GradientDrawable border = new android.graphics.drawable.GradientDrawable();
            border.setColor(0xff100b16); border.setCornerRadius(6 * density); border.setStroke(Math.max(1, Math.round(density)), 0xffff3d8b);
            content.setBackground(border);
            int padding = Math.round(4 * density); content.setPadding(padding,padding,padding,padding);
            loupeImage = new ImageView(activity); loupeImage.setScaleType(ImageView.ScaleType.FIT_CENTER);
            loupeTime = new TextView(activity); loupeTime.setTextColor(0xffff6ea8); loupeTime.setGravity(Gravity.CENTER); loupeTime.setTextSize(13);
            content.addView(loupeImage, new LinearLayout.LayoutParams(-1,0,1)); content.addView(loupeTime);
            loupe = new PopupWindow(content, request.getInt("w"), request.getInt("h"), false);
            loupe.setTouchable(false); loupe.setElevation(12); loupe.setClippingEnabled(true);
        }
        int x = request.getInt("x"), y = request.getInt("y");
        if (loupe.isShowing()) loupe.update(x,y,request.getInt("w"),request.getInt("h"));
        else loupe.showAtLocation(root, Gravity.TOP | Gravity.LEFT, x,y);
        long time = Math.max(0, request.optLong("time"));
        String label = request.optString("label");
        PreviewRequest previous = previewRequest;
        if (previous != null && previous.time == time && previous.label.equals(label)) return;
        if (previous == null) {
            loupeImage.setImageDrawable(null);
            loupeTime.setText("Loading frame…");
        }
        previewRequest = new PreviewRequest(new JSONObject(selected.toString()),
            previewGeneration.get(), time, label);
        startPreviewWorker();
    }

    private void endPreview() {
        previewGeneration.incrementAndGet();
        previewRequest = null;
        if (loupe != null) loupe.dismiss();
        if (loupeImage != null) loupeImage.setImageDrawable(null);
    }

    private void startPreviewWorker() {
        if (!previewBusy.compareAndSet(false,true)) return;
        previews.execute(() -> {
            PreviewRequest completed = null;
            try {
                while (!closed) {
                    PreviewRequest request = previewRequest;
                    if (request == null || request == completed) break;
                    completed = request;
                    try {
                        File image = files.thumbnail(request.item, request.time, 384);
                        Bitmap bitmap = BitmapFactory.decodeFile(image.getAbsolutePath());
                        if (bitmap == null) throw new IOException("Could not decode preview");
                        main.post(() -> {
                            // Publish progress within this gesture, then decode the latest request.
                            // Rejecting every superseded time starves slow decoders while dragging.
                            if (request.session == previewGeneration.get() && loupe != null && loupe.isShowing()) {
                                loupeImage.setImageBitmap(bitmap);
                                loupeTime.setText(request.label);
                            } else bitmap.recycle();
                        });
                    } catch (Exception error) {
                        android.util.Log.w("Luma", "Scrub preview unavailable", error);
                        main.post(() -> {
                            if (previewRequest == request && request.session == previewGeneration.get()) {
                                loupeImage.setImageDrawable(null);
                                loupeTime.setText("Preview unavailable");
                            }
                        });
                    }
                }
            } finally {
                previewBusy.set(false);
                if (!closed && previewRequest != null && completed != previewRequest) startPreviewWorker();
            }
        });
    }

    private void ranges(JSONArray ranges, JSONArray sources) throws JSONException {
        int count = ranges == null ? 0 : ranges.length();
        long[] in = new long[count], out = new long[count];
        int[] from = new int[count];
        for (int i = 0; i < count; i++) {
            JSONArray range = ranges.getJSONArray(i);
            in[i] = range.getLong(0); out[i] = range.getLong(1); from[i] = range.optInt(2, 0);
        }
        String[] uris = new String[sources == null ? 0 : sources.length()];
        for (int i = 0; i < uris.length; i++) uris[i] = sources.getString(i);
        rangeIn = in; rangeOut = out; rangeSource = from; sourceUris = uris; clockTime = -1;
        segment = Math.min(segment, Math.max(0, count - 1));
        String primary = selected == null ? "" : selected.optString("uri");
        if (count == 0 && player != null && !primary.isEmpty() && !loadedUri.equals(primary)) load(primary, 0, false);
    }

    private void locateKeyframe(JSONObject request) {
        JSONObject item = selected;
        if (item == null) return;
        io.execute(() -> {
            try { emit(new JSONObject().put("type", "keyframe").put("uri", item.getString("uri"))
                .put("requested", request.getLong("time")).put("time", files.keyframe(item,request.getLong("time")))); }
            catch (Exception e) { error(e.getMessage()); }
        });
    }

    private void export(JSONObject request, boolean proxy) throws Exception {
        if (exporting) return;
        final JSONObject item = selected;
        if (item == null || !"video".equals(item.optString("kind"))) return;
        final long requestedStart = proxy ? 0 : request.getLong("start");
        final long requestedEnd = proxy ? Math.max(item.optLong("duration"), playback.optLong("duration")) : request.getLong("end");
        final JSONObject crop = proxy ? null : request.optJSONObject("crop");
        final boolean hasCrop = crop != null && (crop.optDouble("left",0)>0 || crop.optDouble("top",0)>0 || crop.optDouble("right",1)<1 || crop.optDouble("bottom",1)<1);
        final boolean exact = !proxy && (request.optBoolean("exact") || hasCrop);
        final boolean keepAudio = proxy || request.optBoolean("audio",true);
        if (requestedEnd <= requestedStart) throw new IOException("The clip must end after it starts");
        exporting = true; exportProgress=0; exportStatus = proxy ? "Building playback proxy" : "Exporting clip"; exportUri="";
        exportCancelled.set(false);
        pause();
        exports.execute(() -> {
            File output = null;
            try (MediaFiles.Source src = files.source(item.getString("uri"))) {
                MediaFiles.CutPoint cut = exact || proxy
                    ? new MediaFiles.CutPoint(requestedStart * 1000, requestedStart * 1000)
                    : files.cutPoint(item, requestedStart);
                long start = cut.presentationUs / 1000;
                long lengthUs = requestedEnd * 1000 - cut.decodeUs;
                long inputSeekUs = exact || proxy ? cut.decodeUs : Math.max(0, cut.decodeUs - 1_000_000);
                File proxyTarget = proxy ? files.cached(MediaFiles.identity(item)+":proxy-v1", ".mp4") : null;
                output = proxy ? new File(proxyTarget.getAbsolutePath()+".pending.mp4") : new File(activity.getCacheDir(), "export-"+System.currentTimeMillis()+".mp4");
                List<String> args = new ArrayList<>(Arrays.asList("-v","error","-nostdin","-y","-threads","4","-ss",MediaFiles.secondsUs(inputSeekUs),"-i",src.path));
                // Fast input seeking can land in an earlier interleave block. FFmpeg's
                // stream-copy output seek compares DTS; trim that preroll without decoding video.
                if (!exact && !proxy && cut.decodeUs > 0)
                    args.addAll(Arrays.asList("-ss", MediaFiles.secondsUs(cut.decodeUs - inputSeekUs)));
                args.addAll(Arrays.asList("-t",MediaFiles.secondsUs(lengthUs),"-map","0:v:0"));
                if (keepAudio) args.addAll(Arrays.asList("-map","0:a?"));
                args.addAll(Arrays.asList("-map_metadata","0"));
                if (exact || proxy) {
                    args.addAll(Arrays.asList("-c:v","libx264","-preset","veryfast","-crf",proxy ? "21" : "18","-threads","4"));
                    if (proxy) args.addAll(Arrays.asList("-vf","scale=w='min(1920,iw)':h=-2:flags=fast_bilinear","-pix_fmt","yuv420p"));
                    else if (hasCrop) {
                        double left=crop.getDouble("left"),top=crop.getDouble("top"),right=crop.getDouble("right"),bottom=crop.getDouble("bottom");
                        if (!Double.isFinite(left+top+right+bottom) || left<0 || top<0 || right>1 || bottom>1 || right-left<0.01 || bottom-top<0.01)
                            throw new IOException("Invalid video crop");
                        String filter=String.format(Locale.ROOT,"crop=w='max(2,trunc(iw*%.8f/2)*2)':h='max(2,trunc(ih*%.8f/2)*2)':x='trunc(iw*%.8f/2)*2':y='trunc(ih*%.8f/2)*2'",right-left,bottom-top,left,top);
                        args.addAll(Arrays.asList("-vf",filter,"-pix_fmt","yuv420p"));
                    }
                } else args.addAll(Arrays.asList("-c:v","copy"));
                args.addAll(Arrays.asList("-c:a","aac","-b:a","256k","-avoid_negative_ts","make_zero","-movflags","+faststart",
                    "-progress","pipe:1","-nostats",output.getAbsolutePath()));
                files.run("ffmpeg", args, 7200, exportProcess, line -> {
                    if (exportCancelled.get()) { Process process=exportProcess.get(); if (process!=null) process.destroyForcibly(); }
                    if (line.startsWith("out_time_us=")) try { exportProgress=Math.min(0.99,Long.parseLong(line.substring(12))/(double)lengthUs); } catch (Exception ignored) { }
                });
                if (exportCancelled.get()) throw new InterruptedException("Export cancelled");
                if (output.length() < 100) throw new IOException("Export produced no media");
                if (proxy) {
                    if (!output.renameTo(proxyTarget)) throw new IOException("Could not finish playback proxy");
                    final File ready = proxyTarget; output = null;
                    final long position = playback.optLong("position");
                    JSONObject proxied = new JSONObject(item.toString());
                    proxied.put("uri",Uri.fromFile(ready).toString()).put("width",0).put("height",0).put("proxy",true);
                    main.post(() -> { try { if (selected != null && selected.optString("uri").equals(item.optString("uri"))) {
                        open(proxied); selected=item; if(player!=null) player.setTime(position,false);
                        emit(new JSONObject().put("type","proxy").put("original",item.optString("uri")));
                    } } catch(Exception e) {error(e.getMessage());} });
                    exportStatus="Playback proxy ready";
                } else {
                    String base = item.optString("name","clip").replaceFirst("\\.[^.]+$", "");
                    exportUri = publish(output, base + "_clip_" + System.currentTimeMillis() + ".mp4").toString();
                    exportStatus="Saved to Movies/Luma";
                    emit(new JSONObject().put("type","exported").put("uri",exportUri).put("start",start).put("end",requestedEnd).put("exact",exact));
                    scan();
                }
                exportProgress=1;
            } catch (Exception e) {
                exportStatus=exportCancelled.get() ? "Export cancelled" : "Export failed";
                if(!exportCancelled.get()) error("Export: "+e.getMessage());
            } finally {
                if (output!=null) output.delete();
                exporting=false;
                files.pruneCache(Collections.emptySet());
            }
        });
    }

    /** Copies a finished render into Movies/Luma; a failed copy leaves no pending entry. */
    private Uri publish(File output, String name) throws Exception {
        ContentResolver resolver = activity.getContentResolver();
        ContentValues values = new ContentValues();
        values.put(MediaStore.MediaColumns.DISPLAY_NAME,name); values.put(MediaStore.MediaColumns.MIME_TYPE,"video/mp4");
        values.put(MediaStore.MediaColumns.RELATIVE_PATH,"Movies/Luma"); values.put(MediaStore.MediaColumns.IS_PENDING,1);
        Uri pending = resolver.insert(MediaStore.Video.Media.EXTERNAL_CONTENT_URI,values);
        if (pending == null) throw new IOException("Could not create the exported clip");
        try {
            try (InputStream input = new FileInputStream(output); OutputStream target = resolver.openOutputStream(pending)) {
                if (target == null) throw new IOException("Export destination is unavailable");
                byte[] buffer = new byte[1024*1024]; int count;
                while ((count=input.read(buffer))!=-1) { if(exportCancelled.get()) throw new InterruptedException("Export cancelled"); target.write(buffer,0,count); }
            }
            values.clear(); values.put(MediaStore.MediaColumns.IS_PENDING,0);
            resolver.update(pending,values,null,null);
            return pending;
        } catch (Exception e) {
            resolver.delete(pending,null,null);
            throw e;
        }
    }

    /** Runs the editor's FFmpeg arguments against the edit's videos and publishes the result. */
    private void studioExport(JSONObject request) throws Exception {
        if (exporting) { error("Wait for the current export to finish"); return; }
        final JSONObject item = selected;
        if (item == null || !item.optString("uri").equals(request.optString("uri"))) { error("Open the video before exporting"); return; }
        final JSONArray args = request.getJSONArray("args");
        final JSONArray uris = request.optJSONArray("sources");
        final long lengthUs = Math.max(1, request.optLong("duration")) * 1000;
        exporting = true; exportProgress = 0; exportStatus = "Rendering video"; exportUri = "";
        exportCancelled.set(false);
        pause();
        exports.execute(() -> {
            File output = null;
            Map<Integer, MediaFiles.Source> opened = new HashMap<>();
            try {
                output = new File(activity.getCacheDir(), "studio-" + System.currentTimeMillis() + ".mp4");
                List<String> command = files.command(args, uris, item.getString("uri"), opened, output.getAbsolutePath());
                files.run("ffmpeg", command, 7200, exportProcess, line -> {
                    if (exportCancelled.get()) { Process process=exportProcess.get(); if (process!=null) process.destroyForcibly(); }
                    if (line.startsWith("out_time_us=")) try { exportProgress=Math.min(0.99,Long.parseLong(line.substring(12))/(double)lengthUs); } catch (Exception ignored) { }
                });
                if (exportCancelled.get()) throw new InterruptedException("Export cancelled");
                if (output.length() < 100) throw new IOException("Export produced no media");
                String base = item.optString("name","video").replaceFirst("\\.[^.]+$", "");
                exportUri = publish(output, base + "_edit_" + System.currentTimeMillis() + ".mp4").toString();
                exportStatus = "Saved to Movies/Luma";
                emit(new JSONObject().put("type","exported").put("uri",exportUri).put("studio",true));
                scan();
                exportProgress = 1;
            } catch (Exception e) {
                exportStatus = exportCancelled.get() ? "Export cancelled" : "Export failed";
                if (!exportCancelled.get()) error("Export: " + e.getMessage());
            } finally {
                for (MediaFiles.Source src : opened.values()) try { src.close(); } catch (Exception ignored) { }
                if (output != null) output.delete();
                exporting = false;
                files.pruneCache(Collections.emptySet());
            }
        });
    }

    private void share(String value) {
        if (value.isEmpty() && selected != null) value = selected.optString("uri");
        if (!value.startsWith("content:")) { error("Only original or exported library files can be shared"); return; }
        Uri uri=Uri.parse(value);
        Intent send=new Intent(Intent.ACTION_SEND).setType(activity.getContentResolver().getType(uri));
        send.putExtra(Intent.EXTRA_STREAM,uri).addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION);
        send.setClipData(ClipData.newRawUri("Media",uri));
        activity.startActivity(Intent.createChooser(send,"Share"));
    }

    private void shareMany(JSONArray values) {
        ArrayList<Uri> uris = new ArrayList<>();
        Set<String> families = new HashSet<>();
        for (int i = 0; i < values.length(); i++) {
            String value = values.optString(i);
            if (!value.startsWith("content:")) continue;
            Uri uri = Uri.parse(value);
            String type = activity.getContentResolver().getType(uri);
            families.add(type == null || !type.contains("/") ? "*" : type.substring(0, type.indexOf('/')));
            uris.add(uri);
        }
        if (uris.isEmpty()) { error("Only library files can be shared"); return; }
        if (uris.size() == 1) { share(uris.get(0).toString()); return; }
        Intent send = new Intent(Intent.ACTION_SEND_MULTIPLE).setType(families.size() == 1 ? families.iterator().next() + "/*" : "*/*");
        send.putParcelableArrayListExtra(Intent.EXTRA_STREAM, uris).addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION);
        ClipData clip = ClipData.newRawUri("Media", uris.get(0));
        for (int i = 1; i < uris.size(); i++) clip.addItem(new ClipData.Item(uris.get(i)));
        send.setClipData(clip);
        activity.startActivity(Intent.createChooser(send, "Share " + uris.size() + " items"));
    }

    private void diagnostics() {
        try {
            JSONArray codecs = new JSONArray();
            for(MediaCodecInfo codec:new MediaCodecList(MediaCodecList.ALL_CODECS).getCodecInfos()) {
                if(codec.isEncoder()) continue;
                for(String mime:codec.getSupportedTypes()) if(mime.equals("video/avc")||mime.equals("video/hevc"))
                    codecs.put(new JSONObject().put("name",codec.getName()).put("mime",mime).put("hardware",codec.isHardwareAccelerated()));
            }
            emit(new JSONObject().put("type","diagnostics").put("codecs",codecs).put("model",Build.MANUFACTURER+" "+Build.MODEL));
        } catch(Exception e) { error(e.getMessage()); }
    }

    void close() {
        edits.close(); qwen.close(); waveform.close(); frames.close(); speech.close();
        closed=true; main.removeCallbacks(tick); main.removeCallbacks(rangeWatch); closeMedia();
        Process process=exportProcess.get(); if(process!=null) process.destroyForcibly();
        CompletableFuture<Boolean> answer=consent; if(answer!=null) answer.complete(false);
        manager.cancelled.set(true);
        try{activity.unregisterReceiver(noisy);}catch(Exception ignored){}
        incoming.shutdownNow(); io.shutdownNow(); thumbs.shutdownNow(); previews.shutdownNow(); photos.shutdownNow(); exports.shutdownNow(); changes.shutdownNow();
        if(vlc!=null) vlc.release();
    }
}
