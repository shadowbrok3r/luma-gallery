package app.luma.gallery;

import android.app.PendingIntent;
import android.app.RecoverableSecurityException;
import android.content.ContentResolver;
import android.content.ContentUris;
import android.content.ContentValues;
import android.content.Context;
import android.content.Intent;
import android.content.IntentSender;
import android.content.pm.PackageManager;
import android.net.Uri;
import android.os.Build;
import android.provider.DocumentsContract;
import android.provider.MediaStore;
import android.webkit.MimeTypeMap;
import org.json.JSONArray;
import org.json.JSONObject;
import java.io.*;
import java.util.*;
import java.util.concurrent.Callable;
import java.util.concurrent.CancellationException;
import java.util.concurrent.atomic.AtomicBoolean;

/** Library changes: MediaStore items through system consent, linked-folder documents directly. */
final class MediaManager {
    interface Consent { boolean grant(IntentSender request) throws Exception; }
    interface Progress { void update(String status, double fraction); }
    private interface Change { void apply(JSONObject item) throws Exception; }

    static final class Result {
        int done, failed;
        boolean cancelled;
        String error = "";
        final JSONArray uris = new JSONArray();
        final JSONObject renamed = new JSONObject();
    }

    private static final String READ_ONLY = "Luma can only read this linked folder. Add it again to allow changes.";
    final AtomicBoolean cancelled = new AtomicBoolean();
    private final Context context;
    private final ContentResolver resolver;

    MediaManager(Context context) {
        this.context = context;
        resolver = context.getContentResolver();
    }

    static boolean inMediaStore(JSONObject item) {
        return MediaStore.AUTHORITY.equals(Uri.parse(item.optString("uri")).getAuthority());
    }

    /** Image or video collection URI for a MediaStore file-table URI. */
    static Uri typed(JSONObject item) {
        Uri uri = Uri.parse(item.optString("uri"));
        List<String> path = uri.getPathSegments();
        if (!MediaStore.AUTHORITY.equals(uri.getAuthority()) || path.size() != 3 || !"file".equals(path.get(1))) return uri;
        Uri collection = "video".equals(item.optString("kind"))
            ? MediaStore.Video.Media.getContentUri(path.get(0)) : MediaStore.Images.Media.getContentUri(path.get(0));
        return ContentUris.withAppendedId(collection, ContentUris.parseId(uri));
    }

    static List<Uri> typed(List<JSONObject> items) {
        List<Uri> uris = new ArrayList<>();
        for (JSONObject item : items) uris.add(typed(item));
        return uris;
    }

    Result run(JSONObject request, Consent consent, Progress progress) throws Exception {
        cancelled.set(false);
        String action = request.getString("action"), target = request.optString("target");
        JSONArray array = request.getJSONArray("items");
        List<JSONObject> media = new ArrayList<>(), documents = new ArrayList<>();
        for (int i = 0; i < array.length(); i++) {
            JSONObject item = array.getJSONObject(i);
            (inMediaStore(item) ? media : documents).add(item);
        }
        Result result = new Result();
        switch (action) {
            case "trash":
            case "restore":
                if (Build.VERSION.SDK_INT < 30) throw new IOException("Trash requires Android 11 or newer");
                if (!grant(consent, media, result, () -> MediaStore.createTrashRequest(resolver, typed(media), "trash".equals(action)))) return result;
                for (JSONObject item : media) done(result, item);
                // Linked-folder documents are deleted permanently.
                each(documents, "Deleting", progress, result, this::deleteDocument);
                break;
            case "delete":
                if (Build.VERSION.SDK_INT >= 30) {
                    if (!grant(consent, media, result, () -> MediaStore.createDeleteRequest(resolver, typed(media)))) return result;
                    for (JSONObject item : media) done(result, item);
                } else each(media, "Deleting", progress, result, item -> {
                    if (withConsent(consent, () -> resolver.delete(typed(item), null, null)) == 0)
                        throw new IOException("It is no longer available");
                });
                each(documents, "Deleting", progress, result, this::deleteDocument);
                break;
            case "rename": {
                JSONObject names = request.getJSONObject("names");
                if (!grantWrite(consent, media, result)) return result;
                each(media, "Renaming", progress, result,
                    item -> update(consent, item, MediaStore.MediaColumns.DISPLAY_NAME, names.getString(item.getString("uri"))));
                each(documents, "Renaming", progress, result, item -> {
                    Uri renamed = writable(() -> DocumentsContract.renameDocument(resolver, Uri.parse(item.getString("uri")), names.getString(item.getString("uri"))));
                    if (renamed == null) throw new IOException("The folder refused the new name");
                    result.renamed.put(item.getString("uri"), renamed.toString());
                });
                break;
            }
            case "move":
            case "rename_album": {
                List<JSONObject> moving = new ArrayList<>();
                for (JSONObject item : media) {
                    if (item.optString("album").equalsIgnoreCase(target)) done(result, item);
                    else moving.add(item);
                }
                if (!grantWrite(consent, moving, result)) return result;
                each(moving, "Moving", progress, result,
                    item -> update(consent, item, MediaStore.MediaColumns.RELATIVE_PATH, target + "/"));
                if ("rename_album".equals(action)) renameFolder(documents, target, result);
                else each(documents, "Moving", progress, result, item -> {
                    if (context.checkCallingOrSelfUriPermission(Uri.parse(item.getString("uri")), Intent.FLAG_GRANT_WRITE_URI_PERMISSION)
                        != PackageManager.PERMISSION_GRANTED) throw new IOException(READ_ONLY);
                    Uri copy = copy(item, target);
                    deleteDocument(item);
                    result.renamed.put(item.getString("uri"), copy.toString());
                });
                break;
            }
            case "copy": {
                List<JSONObject> all = new ArrayList<>(media);
                all.addAll(documents);
                each(all, "Copying", progress, result, item -> copy(item, target));
                break;
            }
            default:
                throw new IOException("Unknown change: " + action);
        }
        return result;
    }

    private boolean grant(Consent consent, List<JSONObject> items, Result result, Callable<PendingIntent> request) throws Exception {
        if (items.isEmpty() || consent.grant(request.call().getIntentSender())) return true;
        result.cancelled = true;
        return false;
    }

    private boolean grantWrite(Consent consent, List<JSONObject> items, Result result) throws Exception {
        return Build.VERSION.SDK_INT < 30 || grant(consent, items, result, () -> MediaStore.createWriteRequest(resolver, typed(items)));
    }

    /** Retries a change after Android 10's per-item consent. */
    private <T> T withConsent(Consent consent, Callable<T> change) throws Exception {
        try { return change.call(); }
        catch (RecoverableSecurityException e) {
            if (!consent.grant(e.getUserAction().getActionIntent().getIntentSender())) throw new CancellationException();
            return change.call();
        }
    }

    private static <T> T writable(Callable<T> change) throws Exception {
        try { return change.call(); }
        catch (SecurityException e) { throw new IOException(READ_ONLY); }
    }

    private void each(List<JSONObject> items, String verb, Progress progress, Result result, Change change) {
        for (int i = 0; i < items.size() && !result.cancelled; i++) {
            if (cancelled.get()) { result.cancelled = true; return; }
            JSONObject item = items.get(i);
            progress.update(verb + " " + (i + 1) + " of " + items.size(), i / (double) items.size());
            try {
                change.apply(item);
                done(result, item);
            } catch (CancellationException e) {
                result.cancelled = true;
            } catch (Exception e) {
                result.failed++;
                if (result.error.isEmpty()) result.error = item.optString("name") + ": " + e.getMessage();
                android.util.Log.w("Luma", verb + " " + item.optString("uri"), e);
            }
        }
    }

    private static void done(Result result, JSONObject item) {
        result.done++;
        result.uris.put(item.optString("uri"));
    }

    private void update(Consent consent, JSONObject item, String column, String value) throws Exception {
        ContentValues values = new ContentValues();
        values.put(column, value);
        if (withConsent(consent, () -> resolver.update(typed(item), values, null, null)) == 0)
            throw new IOException("It is no longer available");
    }

    private void deleteDocument(JSONObject item) throws Exception {
        if (!writable(() -> DocumentsContract.deleteDocument(resolver, Uri.parse(item.getString("uri")))))
            throw new IOException("The folder refused the deletion");
    }

    /** Renames a linked subfolder and maps its files' path-based document IDs. */
    private void renameFolder(List<JSONObject> documents, String target, Result result) {
        if (documents.isEmpty()) return;
        try {
            String folder = documents.get(0).optString("folder");
            if (folder.isEmpty()) throw new IOException("A linked folder itself cannot be renamed");
            Uri before = Uri.parse(folder);
            Uri after = writable(() -> DocumentsContract.renameDocument(resolver, before, target.substring(target.lastIndexOf('/') + 1)));
            if (after == null) throw new IOException("The folder refused the new name");
            String from = DocumentsContract.getDocumentId(before) + "/", to = DocumentsContract.getDocumentId(after) + "/";
            for (JSONObject item : documents) {
                Uri uri = Uri.parse(item.getString("uri"));
                String id = DocumentsContract.getDocumentId(uri);
                if (id.startsWith(from))
                    result.renamed.put(uri.toString(), DocumentsContract.buildDocumentUriUsingTree(uri, to + id.substring(from.length())).toString());
                done(result, item);
            }
        } catch (Exception e) {
            result.failed += documents.size();
            if (result.error.isEmpty()) result.error = String.valueOf(e.getMessage());
        }
    }

    /** Copies an item into a MediaStore album. */
    private Uri copy(JSONObject item, String album) throws Exception {
        Uri source = Uri.parse(item.getString("uri"));
        String name = item.optString("name");
        boolean video = "video".equals(item.optString("kind"));
        String mime = resolver.getType(source);
        if (mime == null || !(mime.startsWith("image/") || mime.startsWith("video/"))) {
            String extension = name.substring(name.lastIndexOf('.') + 1).toLowerCase(Locale.ROOT);
            mime = MimeTypeMap.getSingleton().getMimeTypeFromExtension(extension);
            if (mime == null) mime = video ? "video/mp4" : "image/jpeg";
        }
        ContentValues values = new ContentValues();
        values.put(MediaStore.MediaColumns.DISPLAY_NAME, name);
        values.put(MediaStore.MediaColumns.MIME_TYPE, mime);
        values.put(MediaStore.MediaColumns.RELATIVE_PATH, album + "/");
        values.put(MediaStore.MediaColumns.IS_PENDING, 1);
        Uri collection = video ? MediaStore.Video.Media.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY)
            : MediaStore.Images.Media.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY);
        Uri copy = resolver.insert(collection, values);
        if (copy == null) throw new IOException("Could not create the copy");
        try (InputStream input = resolver.openInputStream(source); OutputStream output = resolver.openOutputStream(copy)) {
            if (input == null || output == null) throw new IOException("Media is unavailable");
            byte[] buffer = new byte[1 << 20];
            for (int count; (count = input.read(buffer)) != -1; ) {
                if (cancelled.get()) throw new CancellationException();
                output.write(buffer, 0, count);
            }
        } catch (Exception e) {
            resolver.delete(copy, null, null);
            throw e;
        }
        values.clear();
        values.put(MediaStore.MediaColumns.IS_PENDING, 0);
        resolver.update(copy, values, null, null);
        return copy;
    }
}
