package app.luma.shareprobe;

import android.content.*;
import android.database.Cursor;
import android.database.MatrixCursor;
import android.net.Uri;
import android.os.ParcelFileDescriptor;
import android.provider.OpenableColumns;
import java.io.*;

/** Test-only, unexported provider: fixtures exist solely in another app's private storage. */
public final class SharedProvider extends ContentProvider {
    @Override public boolean onCreate() {
        for (String name : new String[]{"Shared-Photo.png", "Shared-Second.webp", "Shared-Clip.mp4", "Note.txt"}) {
            File file = new File(getContext().getFilesDir(), name);
            try (InputStream in = getContext().getAssets().open(name); OutputStream out = new FileOutputStream(file)) {
                byte[] buffer = new byte[8192];
                for (int n; (n = in.read(buffer)) > 0;) out.write(buffer, 0, n);
            } catch (IOException e) { throw new IllegalStateException(e); }
        }
        return true;
    }
    private String name(Uri uri) {
        String name = uri.getLastPathSegment();
        if ("opaque".equals(name)) return "Shared-Photo.png";
        if ("slow".equals(name)) return "Shared-Second.webp";
        return name == null ? "missing" : name;
    }
    @Override public String getType(Uri uri) {
        String name = name(uri);
        return name.endsWith(".mp4") ? "video/mp4" : name.endsWith(".webp") ? "image/webp" :
            name.endsWith(".txt") ? "text/plain" : "image/png";
    }
    @Override public Cursor query(Uri uri, String[] projection, String selection, String[] args, String sort) {
        if ("slow".equals(uri.getLastPathSegment())) {
            try { Thread.sleep(5000); } catch (InterruptedException e) { Thread.currentThread().interrupt(); }
        }
        String[] columns = projection == null ? new String[]{OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE} : projection;
        MatrixCursor result = new MatrixCursor(columns);
        MatrixCursor.RowBuilder row = result.newRow();
        for (String column : columns) {
            if (column.equals(OpenableColumns.DISPLAY_NAME)) row.add("opaque".equals(uri.getLastPathSegment()) ? "Shared without extension" : name(uri));
            else if (column.equals(OpenableColumns.SIZE)) row.add(new File(getContext().getFilesDir(), name(uri)).length());
            else row.add(null);
        }
        return result;
    }
    @Override public ParcelFileDescriptor openFile(Uri uri, String mode) throws FileNotFoundException {
        if (!"r".equals(mode) || !new java.util.HashSet<>(java.util.Arrays.asList(
                "Shared-Photo.png", "Shared-Second.webp", "Shared-Clip.mp4", "Note.txt")).contains(name(uri)))
            throw new FileNotFoundException("Unavailable probe fixture");
        return ParcelFileDescriptor.open(new File(getContext().getFilesDir(), name(uri)), ParcelFileDescriptor.MODE_READ_ONLY);
    }
    @Override public Uri insert(Uri uri, ContentValues values) { throw new UnsupportedOperationException(); }
    @Override public int delete(Uri uri, String selection, String[] args) { throw new UnsupportedOperationException(); }
    @Override public int update(Uri uri, ContentValues values, String selection, String[] args) { throw new UnsupportedOperationException(); }
}
