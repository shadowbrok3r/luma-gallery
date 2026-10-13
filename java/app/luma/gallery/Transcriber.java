package app.luma.gallery;

import android.content.Context;
import org.json.*;
import java.io.*;
import java.net.HttpURLConnection;
import java.net.URL;
import java.security.MessageDigest;
import java.util.*;
import java.util.concurrent.*;
import java.util.concurrent.atomic.*;
import java.util.function.Consumer;
import java.util.regex.*;

/** Recognizes the edit's speech on the phone with whisper.cpp; the model is downloaded once and checked. */
final class Transcriber {
    private static final String MODEL = "ggml-base-q5_1.bin";
    private static final String MODEL_URL = "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/" + MODEL;
    private static final String MODEL_SHA256 = "422f1ae452ade6f30a004d7e5c6a43195e4433bc370bf23fac9cc591f01a8898";
    private static final long MODEL_SIZE = 59_707_625L;
    private static final Pattern PROGRESS = Pattern.compile("progress = +(\\d+)%");
    private final Context context;
    private final MediaFiles files;
    private final Consumer<JSONObject> emit;
    private final ExecutorService worker = Executors.newSingleThreadExecutor();
    private final AtomicReference<Process> process = new AtomicReference<>();
    private final AtomicBoolean cancelled = new AtomicBoolean();
    volatile boolean busy, modelReady;
    volatile String status = "";
    volatile double progress;

    Transcriber(Context context, MediaFiles files, Consumer<JSONObject> emit) {
        this.context = context; this.files = files; this.emit = emit;
        modelReady = model().length() == MODEL_SIZE;
    }

    private File model() { return new File(new File(context.getFilesDir(), "speech"), MODEL); }

    void cancel() {
        cancelled.set(true);
        Process running = process.get();
        if (running != null) running.destroyForcibly();
    }

    void deleteModel() {
        if (busy) return;
        model().delete();
        modelReady = false;
    }

    /** Writes the request's FFmpeg audio job to WAV, recognizes it and emits the path of whisper's JSON. */
    void run(JSONObject request) {
        if (busy) return;
        busy = true; cancelled.set(false); progress = 0; status = "Starting";
        worker.execute(() -> {
            File dir = new File(context.getCacheDir(), "speech");
            File wav = null;
            Map<Integer, MediaFiles.Source> opened = new HashMap<>();
            try {
                if (!modelReady) download();
                File[] stale = dir.listFiles();
                if (stale != null) for (File file : stale) file.delete();
                dir.mkdirs();
                long stamp = System.currentTimeMillis();
                wav = new File(dir, "audio-" + stamp + ".wav");
                String base = new File(dir, "words-" + stamp).getAbsolutePath();
                long lengthUs = Math.max(1, request.optLong("duration")) * 1000;
                status = "Reading the audio"; progress = 0;
                List<String> command = files.command(request.getJSONArray("args"), request.optJSONArray("sources"), "", opened, wav.getAbsolutePath());
                files.run("ffmpeg", command, 1800, process, line -> {
                    stopIfCancelled();
                    if (line.startsWith("out_time_us=")) try { progress = Math.min(0.99, Long.parseLong(line.substring(12)) / (double) lengthUs); } catch (NumberFormatException ignored) { }
                });
                if (cancelled.get()) throw new InterruptedException();
                status = "Recognizing speech"; progress = 0;
                String threads = String.valueOf(Math.max(2, Math.min(4, Runtime.getRuntime().availableProcessors())));
                files.run("whisper", Arrays.asList("-m", model().getAbsolutePath(), "-f", wav.getAbsolutePath(),
                    "-l", request.optString("language", "auto"), "-t", threads, "-ojf", "-sns", "-pp", "-of", base), 3600, process, line -> {
                    stopIfCancelled();
                    Matcher found = PROGRESS.matcher(line);
                    if (found.find()) progress = Integer.parseInt(found.group(1)) / 100.0;
                });
                if (cancelled.get()) throw new InterruptedException();
                File json = new File(base + ".json");
                if (!json.isFile()) throw new IOException("Speech recognition produced no result");
                emit.accept(new JSONObject().put("type", "transcript").put("path", json.getAbsolutePath()));
            } catch (Exception e) {
                android.util.Log.w("Luma", "Transcription", e);
                String message = cancelled.get() ? "Transcription cancelled" : e.getMessage();
                try { emit.accept(new JSONObject().put("type", "transcript_error").put("message", message)); } catch (JSONException ignored) { }
            } finally {
                for (MediaFiles.Source source : opened.values()) try { source.close(); } catch (IOException ignored) { }
                if (wav != null) wav.delete();
                busy = false; status = ""; progress = 0;
            }
        });
    }

    private void stopIfCancelled() {
        if (!cancelled.get()) return;
        Process running = process.get();
        if (running != null) running.destroyForcibly();
    }

    private void download() throws Exception {
        status = "Downloading the speech model"; progress = 0;
        File target = model();
        target.getParentFile().mkdirs();
        File part = new File(target.getPath() + ".part");
        MessageDigest sha = MessageDigest.getInstance("SHA-256");
        try {
            HttpURLConnection connection = (HttpURLConnection) new URL(MODEL_URL).openConnection();
            connection.setConnectTimeout(20_000);
            connection.setReadTimeout(30_000);
            try {
                int code = connection.getResponseCode();
                if (code != 200) throw new IOException("The speech model download failed (HTTP " + code + ")");
                try (InputStream in = connection.getInputStream(); OutputStream out = new FileOutputStream(part)) {
                    byte[] buffer = new byte[1 << 16];
                    long total = 0;
                    for (int n; (n = in.read(buffer)) > 0; ) {
                        if (cancelled.get()) throw new InterruptedException();
                        out.write(buffer, 0, n);
                        sha.update(buffer, 0, n);
                        total += n;
                        progress = Math.min(1, total / (double) MODEL_SIZE);
                        status = String.format(Locale.ROOT, "Downloading the speech model (%d of %d MB)", total >> 20, MODEL_SIZE >> 20);
                    }
                }
            } finally {
                connection.disconnect();
            }
        } catch (IOException e) {
            part.delete();
            throw new IOException("Could not download the speech model. Check the internet connection and try again.", e);
        } catch (InterruptedException e) {
            part.delete();
            throw e;
        }
        StringBuilder hex = new StringBuilder();
        for (byte b : sha.digest()) hex.append(String.format(Locale.ROOT, "%02x", b));
        if (part.length() != MODEL_SIZE || !hex.toString().equals(MODEL_SHA256)) {
            part.delete();
            throw new IOException("The downloaded speech model was damaged. Try again.");
        }
        if (!part.renameTo(target)) throw new IOException("Could not store the speech model");
        modelReady = true;
    }

    void close() { cancel(); worker.shutdownNow(); }
}
