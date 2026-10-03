package app.luma.gallery;

import android.content.ContentResolver;
import android.content.ContentUris;
import android.content.Context;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.database.Cursor;
import android.graphics.Bitmap;
import android.graphics.BitmapFactory;
import android.media.MediaMetadataRetriever;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.os.ParcelFileDescriptor;
import android.provider.DocumentsContract;
import android.provider.MediaStore;
import android.util.Size;
import org.json.JSONArray;
import org.json.JSONObject;
import java.io.*;
import java.nio.charset.StandardCharsets;
import java.security.MessageDigest;
import java.util.*;
import java.util.concurrent.*;
import java.util.concurrent.atomic.AtomicReference;
import java.util.function.Consumer;

final class MediaFiles {
    private final Context context;
    private final File cache;
    private static final ScheduledExecutorService watchdog = Executors.newSingleThreadScheduledExecutor();
    MediaFiles(Context context) {
        this.context = context;
        cache = new File(context.getCacheDir(), "media");
        cache.mkdirs();
    }

    static final class Source implements AutoCloseable {
        final ParcelFileDescriptor fd;
        final String path;
        Source(ContentResolver resolver, String uri) throws IOException {
            Uri parsed = Uri.parse(uri);
            if (!"content".equals(parsed.getScheme()) && !"file".equals(parsed.getScheme()))
                throw new IOException("Only local media can be opened");
            fd = resolver.openFileDescriptor(parsed, "r");
            if (fd == null) throw new IOException("File is no longer available");
            // Native spawn inherits this authorized descriptor; multi-GB clips are never copied.
            path = "/proc/" + android.os.Process.myPid() + "/fd/" + fd.getFd();
        }
        public void close() throws IOException { fd.close(); }
    }

    Source source(String uri) throws IOException { return new Source(context.getContentResolver(), uri); }

    File cached(String key, String suffix) throws Exception {
        byte[] digest = MessageDigest.getInstance("SHA-256").digest(key.getBytes(StandardCharsets.UTF_8));
        StringBuilder name = new StringBuilder();
        for (int i = 0; i < 16; i++) name.append(String.format(Locale.ROOT, "%02x", digest[i]));
        return new File(cache, name + suffix);
    }

    static String identity(JSONObject item) {
        return item.optString("uri") + ":" + item.optLong("modified") + ":" + item.optLong("size");
    }

    static boolean isDng(JSONObject item) {
        return item.optString("name").toLowerCase(Locale.ROOT).endsWith(".dng");
    }

    String executable(String tool) { return new File(context.getApplicationInfo().nativeLibraryDir, "lib" + tool + "_exec.so").getAbsolutePath(); }

    String run(String tool, List<String> args, int seconds, AtomicReference<Process> running,
               Consumer<String> lineConsumer) throws Exception {
        List<String> command = new ArrayList<>();
        command.add(executable(tool));
        int input = -1;
        String descriptorPrefix = "/proc/" + android.os.Process.myPid() + "/fd/";
        for (String arg : args) {
            if (arg.startsWith(descriptorPrefix)) {
                input = Integer.parseInt(arg.substring(descriptorPrefix.length()));
                command.add(tool.equals("raw") || tool.equals("dng") ? "-" : "fd:");
            }
            else command.add(arg);
        }
        Process process = new MediaProcess(command, input);
        if (running != null) running.set(process);
        ScheduledFuture<?> timeout = watchdog.schedule(process::destroyForcibly, seconds, TimeUnit.SECONDS);
        StringBuilder result = new StringBuilder();
        try (BufferedReader reader = new BufferedReader(new InputStreamReader(process.getInputStream(), StandardCharsets.UTF_8))) {
            String line;
            while ((line = reader.readLine()) != null) {
                if (lineConsumer != null) lineConsumer.accept(line);
                if (result.length() > 2_000_000) result.delete(0, 1_000_000);
                result.append(line).append('\n');
            }
            int exit = process.waitFor();
            if (exit != 0) {
                String detail = result.substring(Math.max(0, result.length() - 1200)).trim();
                throw new IOException(tool + " failed (" + exit + "): " + detail);
            }
            return result.toString();
        } finally {
            timeout.cancel(false);
            if (running != null) running.compareAndSet(process, null);
            process.destroy();
        }
    }

    JSONObject probe(JSONObject item) throws Exception {
        try (Source src = source(item.getString("uri"))) {
            String json = run("ffprobe", Arrays.asList("-v", "error", "-show_streams", "-show_format", "-of", "json", src.path), 30, null, null);
            return new JSONObject(json);
        }
    }

    static final class CutPoint {
        final long presentationUs, decodeUs;
        CutPoint(long presentationUs, long decodeUs) {
            this.presentationUs = presentationUs;
            this.decodeUs = decodeUs;
        }
    }

    long keyframe(JSONObject item, long timeMs) throws Exception {
        return cutPoint(item, timeMs).presentationUs / 1000;
    }

    CutPoint cutPoint(JSONObject item, long timeMs) throws Exception {
        if (timeMs <= 0) return new CutPoint(0, 0);
        long lookback = 0;
        for (int attempt = 0; attempt < 8; attempt++) {
            long seek = Math.max(0, timeMs - lookback);
            try (Source src = source(item.getString("uri"))) {
                JSONObject data = new JSONObject(run("ffprobe", Arrays.asList("-v", "error", "-select_streams", "v:0",
                    "-read_intervals", seconds(seek) + "%+#32", "-show_packets", "-show_entries", "packet=pts_time,dts_time,flags",
                    "-of", "json", src.path), 30, null, null));
                JSONArray packets = data.optJSONArray("packets");
                CutPoint candidate = null;
                if (packets != null) for (int i = 0; i < packets.length(); i++) {
                    JSONObject packet = packets.getJSONObject(i);
                    double pts = packet.optDouble("pts_time", Double.NaN);
                    if (packet.optString("flags").contains("K") && Double.isFinite(pts)) {
                        long at = (long)Math.floor(pts * 1_000_000);
                        double dts = packet.optDouble("dts_time", pts);
                        if (!Double.isFinite(dts)) dts = pts;
                        if (at <= timeMs * 1000 && (candidate == null || at > candidate.presentationUs))
                            candidate = new CutPoint(Math.max(0, at), Math.max(0, (long)Math.floor(dts * 1_000_000)));
                    }
                }
                if (candidate != null) return candidate;
                if (seek == 0) return new CutPoint(0, 0);
            }
            // Demuxers seek by decode time. Reordered Sony B-frames can put that
            // keyframe's presentation time after the requested cut; retry earlier.
            lookback = lookback == 0 ? 500 : lookback * 2;
        }
        throw new IOException("Could not locate a preceding keyframe. Use Exact cut.");
    }

    File thumbnail(JSONObject item, long at, int width) throws Exception {
        File out = cached(identity(item) + ":" + at + ":" + width, ".jpg");
        if (out.length() > 0) { out.setLastModified(System.currentTimeMillis()); return out; }
        File work = new File(cache, out.getName() + "." + Thread.currentThread().getId() + ".pending.jpg");
        try {
            renderThumbnail(item,at,width,work);
            if (!work.renameTo(out)) throw new IOException("Could not finish thumbnail");
            return out;
        } finally { work.delete(); }
    }

    private File renderThumbnail(JSONObject item,long at,int width,File out) throws Exception {
        Bitmap bitmap = null;
        String kind = item.optString("kind");
        Uri uri = Uri.parse(item.getString("uri"));
        if ("video".equals(kind)) {
            try (MediaMetadataRetriever retriever = new MediaMetadataRetriever()) {
                retriever.setDataSource(context, uri);
                bitmap = retriever.getScaledFrameAtTime(Math.max(0, at) * 1000,
                    MediaMetadataRetriever.OPTION_CLOSEST, width, Math.max(1, width * 9 / 16));
            } catch (Exception ignored) { }
        } else if (!"raw".equals(kind)) {
            try { bitmap = context.getContentResolver().loadThumbnail(uri, new Size(width, width), null); }
            catch (Exception ignored) { }
            if (bitmap == null) try (Source src = source(uri.toString())) {
                BitmapFactory.Options bounds = new BitmapFactory.Options();
                bounds.inJustDecodeBounds = true;
                BitmapFactory.decodeFile(src.path, bounds);
                BitmapFactory.Options options = new BitmapFactory.Options();
                int side = Math.max(bounds.outWidth, bounds.outHeight);
                while (side / Math.max(1, options.inSampleSize) > width * 2) options.inSampleSize = Math.max(1, options.inSampleSize) * 2;
                bitmap = BitmapFactory.decodeFile(src.path, options);
            }
        }
        if (bitmap != null) {
            try (FileOutputStream stream = new FileOutputStream(out)) { bitmap.compress(Bitmap.CompressFormat.JPEG, 88, stream); }
            finally { bitmap.recycle(); }
            return out;
        }
        try (Source src = source(uri.toString())) {
            if ("raw".equals(kind)) {
                File embedded = cached(identity(item) + ":" + width, ".embedded");
                try {
                    try {
                        run("raw", Arrays.asList(src.path, embedded.getAbsolutePath(), "preview"), 45, null, null);
                    } catch (Exception error) {
                        if (!isDng(item)) throw error;
                        // Some DNGs have no embedded JPEG. Reopen after the first
                        // decoder consumed stdin, then render a correctly oriented preview.
                        try (Source fallback = source(uri.toString())) {
                            run("dng", Arrays.asList(fallback.path, embedded.getAbsolutePath(), "preview"), 180, null, null);
                        }
                    }
                    run("ffmpeg", Arrays.asList("-v", "error", "-y", "-i", embedded.getAbsolutePath(),
                        "-vf", "scale=" + width + ":-2", "-frames:v", "1", out.getAbsolutePath()), 45, null, null);
                } finally { embedded.delete(); }
            } else {
                run("ffmpeg", Arrays.asList("-v", "error", "-y", "-threads", "2", "-ss", seconds(at), "-i", src.path,
                    "-vf", "scale=" + width + ":-2", "-frames:v", "1", out.getAbsolutePath()), 45, null, null);
            }
        }
        if (out.length() == 0) throw new IOException("No thumbnail available");
        return out;
    }

    File developRaw(JSONObject item, AtomicReference<Process> running) throws Exception {
        File output = cached(identity(item) + (isDng(item) ? ":dng-full-v1" : ":full-v1"), ".jpg");
        if (output.length() > 0) return output;
        File ppm = cached(identity(item), ".ppm");
        File partial = cached(identity(item), ".developing.jpg");
        try (Source src = source(item.getString("uri"))) {
            String report = run(isDng(item) ? "dng" : "raw", Arrays.asList(src.path, ppm.getAbsolutePath(), "full"), 180, running, null);
            if (isDng(item)) android.util.Log.i("LumaDNG", report.trim());
            run("ffmpeg", Arrays.asList("-v", "error", "-y", "-i", ppm.getAbsolutePath(), "-frames:v", "1",
                "-q:v", "1", "-pix_fmt", "yuvj444p", partial.getAbsolutePath()), 90, running, null);
            if (!partial.renameTo(output)) throw new IOException("Could not finish RAW cache");
        } finally { ppm.delete(); partial.delete(); }
        return output;
    }

    static String seconds(long ms) { return String.format(Locale.ROOT, "%.6f", ms / 1000.0); }
    static String secondsUs(long us) { return String.format(Locale.ROOT, "%.6f", us / 1_000_000.0); }

    boolean hasMediaAccess() {
        if (Build.VERSION.SDK_INT >= 33)
            return context.checkSelfPermission("android.permission.READ_MEDIA_IMAGES") == PackageManager.PERMISSION_GRANTED
                || context.checkSelfPermission("android.permission.READ_MEDIA_VIDEO") == PackageManager.PERMISSION_GRANTED
                || (Build.VERSION.SDK_INT >= 34 && context.checkSelfPermission("android.permission.READ_MEDIA_VISUAL_USER_SELECTED") == PackageManager.PERMISSION_GRANTED);
        return context.checkSelfPermission("android.permission.READ_EXTERNAL_STORAGE") == PackageManager.PERMISSION_GRANTED;
    }

    String access() {
        if (Build.VERSION.SDK_INT >= 34 && context.checkSelfPermission("android.permission.READ_MEDIA_IMAGES") != PackageManager.PERMISSION_GRANTED
            && context.checkSelfPermission("android.permission.READ_MEDIA_VIDEO") != PackageManager.PERMISSION_GRANTED
            && context.checkSelfPermission("android.permission.READ_MEDIA_VISUAL_USER_SELECTED") == PackageManager.PERMISSION_GRANTED) return "selected";
        return hasMediaAccess() ? "granted" : "none";
    }

    JSONArray library() throws Exception {
        List<JSONObject> items = new ArrayList<>();
        Set<String> seen = new HashSet<>();
        if (hasMediaAccess()) {
            query(MediaStore.Images.Media.EXTERNAL_CONTENT_URI, null, "photo", items, seen);
            query(MediaStore.Video.Media.EXTERNAL_CONTENT_URI, null, "video", items, seen);
            query(MediaStore.Files.getContentUri("external"),
                "LOWER(_display_name) LIKE '%.arw' OR LOWER(_display_name) LIKE '%.dng'", "raw", items, seen);
        }
        for (android.content.UriPermission permission : context.getContentResolver().getPersistedUriPermissions()) {
            if (!permission.isReadPermission()) continue;
            Uri tree = permission.getUri();
            if (!DocumentsContract.isTreeUri(tree)) continue;
            try { walk(tree, DocumentsContract.getTreeDocumentId(tree), "", items, seen, 0); }
            catch (Exception e) { android.util.Log.w("Luma", "Folder unavailable: " + tree, e); }
        }
        items.sort((a,b) -> Long.compare(b.optLong("modified"), a.optLong("modified")));
        return new JSONArray(items);
    }

    /** Trashed MediaStore items, soonest purge last. */
    JSONArray trash() throws Exception {
        List<JSONObject> items = new ArrayList<>();
        if (Build.VERSION.SDK_INT >= 30 && hasMediaAccess()) {
            Set<String> seen = new HashSet<>();
            Bundle args = new Bundle();
            args.putInt(MediaStore.QUERY_ARG_MATCH_TRASHED, MediaStore.MATCH_ONLY);
            args.putString(ContentResolver.QUERY_ARG_SQL_SORT_ORDER, "date_expires DESC");
            rows(MediaStore.Images.Media.EXTERNAL_CONTENT_URI, args, "photo", items, seen);
            rows(MediaStore.Video.Media.EXTERNAL_CONTENT_URI, args, "video", items, seen);
            items.sort((a,b) -> Long.compare(b.optLong("expires"), a.optLong("expires")));
        }
        return new JSONArray(items);
    }

    void query(Uri collection, String selection, String fallbackKind, List<JSONObject> output, Set<String> seen) throws Exception {
        Bundle args = new Bundle();
        args.putString(ContentResolver.QUERY_ARG_SQL_SELECTION, selection);
        args.putString(ContentResolver.QUERY_ARG_SQL_SORT_ORDER, "date_modified DESC");
        rows(collection, args, fallbackKind, output, seen);
    }

    void rows(Uri collection, Bundle args, String fallbackKind, List<JSONObject> output, Set<String> seen) throws Exception {
        try (Cursor cursor = context.getContentResolver().query(collection, null, args, null)) {
            if (cursor == null) return;
            while (cursor.moveToNext()) {
                String name = string(cursor, "_display_name", "Untitled");
                String kind = kind(name);
                if (kind == null) kind = fallbackKind;
                String relative = string(cursor, "relative_path", "");
                String path = string(cursor, "_data", "");
                String unique = path.isEmpty() ? relative + name : path;
                if (!seen.add(unique)) continue;
                String album = relative.replaceAll("/+$", "");
                if (album.isEmpty()) album = "Device";
                JSONObject item = new JSONObject();
                item.put("uri", ContentUris.withAppendedId(collection, number(cursor, "_id")).toString());
                item.put("name", name).put("kind", kind).put("album", album);
                item.put("size", number(cursor, "_size")).put("modified", number(cursor, "date_modified"));
                item.put("width", number(cursor, "width")).put("height", number(cursor, "height"));
                item.put("duration", number(cursor, "duration"));
                // Purge date for trashed rows only.
                item.put("expires", number(cursor, "is_trashed") == 1 ? Math.max(1, number(cursor, "date_expires")) : 0);
                output.add(item);
            }
        } catch (SecurityException ignored) { }
    }

    void walk(Uri tree, String id, String folder, List<JSONObject> output, Set<String> seen, int depth) throws Exception {
        if (depth > 24) return;
        Uri children = DocumentsContract.buildChildDocumentsUriUsingTree(tree, id);
        String[] columns = {"document_id", "_display_name", "mime_type", "_size", "last_modified"};
        try (Cursor cursor = context.getContentResolver().query(children, columns, null, null, null)) {
            if (cursor == null) return;
            while (cursor.moveToNext()) {
                String childId = cursor.getString(0), name = cursor.getString(1), mime = cursor.getString(2);
                if (DocumentsContract.Document.MIME_TYPE_DIR.equals(mime)) {
                    walk(tree, childId, folder.isEmpty() ? name : folder + "/" + name, output, seen, depth + 1);
                } else {
                    String kind = kind(name);
                    if (kind == null) continue;
                    String uri = DocumentsContract.buildDocumentUriUsingTree(tree, childId).toString();
                    if (!seen.add(uri)) continue;
                    String album = folder;
                    if (album.isEmpty()) { String root = DocumentsContract.getTreeDocumentId(tree); album = root.substring(root.lastIndexOf(':') + 1); }
                    JSONObject item = new JSONObject().put("uri", uri).put("name", name).put("kind", kind)
                        .put("album", album.isEmpty() ? "Folder" : album).put("size", cursor.getLong(3))
                        .put("modified", cursor.getLong(4) / 1000).put("duration", 0).put("width", 0).put("height", 0);
                    if (depth > 0) item.put("folder", DocumentsContract.buildDocumentUriUsingTree(tree, id).toString());
                    output.add(item);
                }
            }
        }
    }

    static String kind(String name) {
        String lower = name.toLowerCase(Locale.ROOT);
        if (lower.endsWith(".arw") || lower.endsWith(".dng")) return "raw";
        if (lower.matches(".*\\.(mp4|mov|m4v|mkv|webm|avi)$")) return "video";
        if (lower.matches(".*\\.(jpe?g|png|webp|heic|heif|avif|bmp)$")) return "photo";
        return null;
    }
    static String string(Cursor cursor, String name, String fallback) {
        int column = cursor.getColumnIndex(name);
        return column < 0 || cursor.isNull(column) ? fallback : cursor.getString(column);
    }
    static long number(Cursor cursor, String name) {
        int column = cursor.getColumnIndex(name);
        return column < 0 || cursor.isNull(column) ? 0 : cursor.getLong(column);
    }

    void pruneCache(Set<String> protectedPaths) {
        File[] files = cache.listFiles();
        if (files == null) return;
        Arrays.sort(files, Comparator.comparingLong(File::lastModified));
        long bytes = 0;
        for (File file : files) bytes += file.length();
        for (File file : files) {
            if (bytes < 1024L * 1024 * 1024) break;
            if (protectedPaths.contains(file.getAbsolutePath()) || file.getName().endsWith(".ppm") || file.getName().contains(".developing")) continue;
            long size = file.length();
            if (file.delete()) bytes -= size;
        }
    }
}
