package app.luma.gallery;

import android.content.ContentResolver;
import android.content.ContentValues;
import android.content.Context;
import android.graphics.Bitmap;
import android.graphics.BitmapFactory;
import android.media.MediaMetadataRetriever;
import android.net.Uri;
import android.os.Build;
import android.provider.MediaStore;
import org.json.JSONObject;
import java.io.*;
import java.util.*;
import java.util.concurrent.*;
import java.util.concurrent.atomic.AtomicReference;
import java.util.function.Consumer;

/** Full-resolution stills decoded from the original video, never from the UI or a proxy. */
final class VideoFrames {
    private final Context context;
    private final MediaFiles files;
    private final Consumer<JSONObject> emit;
    private final Runnable scan;
    private final ExecutorService worker = Executors.newSingleThreadExecutor();
    private final AtomicReference<Process> process = new AtomicReference<>();
    volatile boolean busy, closed;

    VideoFrames(Context context, MediaFiles files, Consumer<JSONObject> emit, Runnable scan) {
        this.context = context; this.files = files; this.emit = emit; this.scan = scan;
    }

    void save(JSONObject item, long time) {
        if (busy || closed) return;
        busy = true;
        worker.execute(() -> {
            File output = null;
            Uri pending = null;
            ContentResolver resolver = context.getContentResolver();
            try {
                output = File.createTempFile("video-frame-", ".png", context.getCacheDir());
                render(item, Math.max(0, time), output);
                if (closed) throw new InterruptedException("Capture cancelled");
                BitmapFactory.Options bounds = new BitmapFactory.Options();
                bounds.inJustDecodeBounds = true;
                BitmapFactory.decodeFile(output.getAbsolutePath(), bounds);
                if (bounds.outWidth <= 0 || bounds.outHeight <= 0) throw new IOException("No video frame was decoded");
                String base = item.optString("name", "Video").replaceFirst("\\.[^.]+$", "")
                    .replaceAll("[\\p{Cntrl}/\\\\]", "_");
                if (base.length() > 100) base = base.substring(0, 100);
                ContentValues values = new ContentValues();
                values.put(MediaStore.MediaColumns.DISPLAY_NAME, base + "_frame_" + time + "ms_" + System.currentTimeMillis() + ".png");
                values.put(MediaStore.MediaColumns.MIME_TYPE, "image/png");
                values.put(MediaStore.MediaColumns.RELATIVE_PATH, "Pictures/Luma");
                values.put(MediaStore.MediaColumns.WIDTH, bounds.outWidth);
                values.put(MediaStore.MediaColumns.HEIGHT, bounds.outHeight);
                values.put(MediaStore.MediaColumns.IS_PENDING, 1);
                pending = resolver.insert(MediaStore.Images.Media.EXTERNAL_CONTENT_URI, values);
                if (pending == null) throw new IOException("Could not create the frame photo");
                try (InputStream in = new FileInputStream(output); OutputStream out = resolver.openOutputStream(pending)) {
                    if (out == null) throw new IOException("Photo destination is unavailable");
                    byte[] buffer = new byte[65536]; int n;
                    while ((n = in.read(buffer)) != -1) {
                        if (closed) throw new InterruptedException("Capture cancelled");
                        out.write(buffer, 0, n);
                    }
                }
                values.clear(); values.put(MediaStore.MediaColumns.IS_PENDING, 0);
                resolver.update(pending, values, null, null);
                Uri saved = pending; pending = null;
                emit.accept(new JSONObject().put("type", "frame_saved").put("uri", saved.toString())
                    .put("source", item.optString("uri")).put("time", time)
                    .put("width", bounds.outWidth).put("height", bounds.outHeight));
                scan.run();
            } catch (Exception | OutOfMemoryError error) {
                android.util.Log.w("Luma", "Frame capture failed", error);
                if (!closed) try {
                    String message = error instanceof OutOfMemoryError
                        ? "Not enough memory to capture this frame" : "Frame capture failed: " + error.getMessage();
                    emit.accept(new JSONObject().put("type", "error").put("message", message));
                } catch (Exception ignored) { }
            } finally {
                if (pending != null) try { resolver.delete(pending, null, null); } catch (Exception ignored) { }
                if (output != null) output.delete();
                busy = false;
            }
        });
    }

    private void render(JSONObject item, long time, File output) throws Exception {
        Bitmap bitmap = null;
        try (MediaMetadataRetriever retriever = new MediaMetadataRetriever()) {
            retriever.setDataSource(context, Uri.parse(item.getString("uri")));
            MediaMetadataRetriever.BitmapParams params = new MediaMetadataRetriever.BitmapParams();
            params.setPreferredConfig(Bitmap.Config.ARGB_8888);
            if (MediaFiles.preciseNativeSeek(retriever)) {
                bitmap = Build.VERSION.SDK_INT >= 30
                    ? retriever.getFrameAtTime(time * 1000, MediaMetadataRetriever.OPTION_CLOSEST, params)
                    : retriever.getFrameAtTime(time * 1000, MediaMetadataRetriever.OPTION_CLOSEST);
            }
        } catch (Exception error) { android.util.Log.w("Luma", "Using software frame capture", error); }
        if (bitmap != null) {
            try (OutputStream out = new FileOutputStream(output)) {
                if (!bitmap.compress(Bitmap.CompressFormat.PNG, 100, out)) throw new IOException("Frame encoding failed");
            } finally { bitmap.recycle(); }
        } else try (MediaFiles.Source src = files.source(item.getString("uri"))) {
            files.run("ffmpeg", Arrays.asList("-v", "error", "-nostdin", "-y", "-threads", "2",
                "-ss", MediaFiles.seconds(time), "-i", src.path, "-map", "0:v:0", "-an", "-sn",
                "-frames:v", "1", "-c:v", "png", "-threads", "2", "-update", "1", output.getAbsolutePath()),
                60, process, null);
        }
    }

    void close() {
        closed = true;
        Process p = process.get(); if (p != null) p.destroyForcibly();
        worker.shutdownNow();
    }
}
