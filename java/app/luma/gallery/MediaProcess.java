package app.luma.gallery;

import android.os.ParcelFileDescriptor;
import java.io.*;
import java.util.List;

/** Passes an already-authorized media descriptor without reopening its protected backing path. */
final class MediaProcess extends Process {
    static { System.loadLibrary("luma_process"); }
    private static native int[] spawn(String[] args, int input) throws IOException;
    private static native int waitChild(int pid, boolean block);
    private final int pid;
    private final InputStream output;
    private Integer result;

    MediaProcess(List<String> args, int input) throws IOException {
        int[] child = spawn(args.toArray(new String[0]), input);
        pid = child[0];
        output = new ParcelFileDescriptor.AutoCloseInputStream(ParcelFileDescriptor.adoptFd(child[1]));
    }
    @Override public InputStream getInputStream() { return output; }
    @Override public InputStream getErrorStream() { return new ByteArrayInputStream(new byte[0]); }
    @Override public OutputStream getOutputStream() { return new ByteArrayOutputStream(); }
    @Override public synchronized int waitFor() {
        if (result == null) result = waitChild(pid, true);
        return result;
    }
    @Override public synchronized int exitValue() {
        if (result == null) { int value = waitChild(pid, false); if (value != Integer.MIN_VALUE) result = value; }
        if (result == null) throw new IllegalThreadStateException("Media process still running");
        return result;
    }
    @Override public void destroy() { if (isAlive()) android.os.Process.sendSignal(pid, 15); }
    @Override public Process destroyForcibly() { if (isAlive()) android.os.Process.sendSignal(pid, 9); return this; }
}
