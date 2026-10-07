package app.luma.shareprobe;

import android.app.Activity;
import android.content.*;
import android.net.Uri;
import android.os.Bundle;
import android.widget.*;
import java.util.*;

public final class ProbeActivity extends Activity {
    @Override public void onCreate(Bundle state) {
        super.onCreate(state);
        LinearLayout layout = new LinearLayout(this);
        layout.setOrientation(LinearLayout.VERTICAL);
        layout.setPadding(20, 80, 20, 20);
        for (String mode : new String[]{"view", "photo", "video", "multiple", "clipdata", "opaque", "partial", "missing", "slow"}) {
            Button button = new Button(this);
            button.setText(mode);
            button.setOnClickListener(v -> send(mode));
            layout.addView(button);
        }
        setContentView(layout);
        if (state == null && getIntent().hasExtra("mode")) send(getIntent().getStringExtra("mode"));
    }
    private void send(String mode) {
        ArrayList<Uri> uris = new ArrayList<>();
        String first = mode.equals("video") ? "Shared-Clip.mp4" : mode.equals("missing") ? "missing" :
            mode.equals("opaque") ? "opaque" : mode.equals("slow") ? "slow" : "Shared-Photo.png";
        uris.add(Uri.parse("content://app.luma.shareprobe.media/" + first));
        boolean multiple = mode.equals("multiple") || mode.equals("partial");
        if (multiple) {
            uris.add(Uri.parse("content://app.luma.shareprobe.media/" + (mode.equals("partial") ? "Note.txt" : "Shared-Clip.mp4")));
            uris.add(Uri.parse("content://app.luma.shareprobe.media/Shared-Second.webp"));
        }
        Intent intent = new Intent(mode.equals("view") ? Intent.ACTION_VIEW : multiple ? Intent.ACTION_SEND_MULTIPLE : Intent.ACTION_SEND);
        String mime = multiple ? "*/*" : mode.equals("video") ? "video/mp4" : "image/png";
        if (mode.equals("view")) intent.setDataAndType(uris.get(0), mime);
        else {
            intent.setType(mime);
            if (multiple) intent.putParcelableArrayListExtra(Intent.EXTRA_STREAM, uris);
            else if (!mode.equals("clipdata")) intent.putExtra(Intent.EXTRA_STREAM, uris.get(0));
        }
        ClipData clip = new ClipData("Probe media", new String[]{mime}, new ClipData.Item(uris.get(0)));
        for (int i = 1; i < uris.size(); i++) clip.addItem(new ClipData.Item(uris.get(i)));
        intent.setClipData(clip);
        intent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION);
        if (getIntent().getBooleanExtra("direct", false)) {
            intent.setPackage("app.luma.gallery");
            startActivity(intent);
        } else if (mode.equals("view")) startActivity(intent);
        else startActivity(Intent.createChooser(intent, "Open in Luma"));
    }
}
