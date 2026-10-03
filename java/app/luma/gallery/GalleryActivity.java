package app.luma.gallery;

import android.content.Intent;
import android.os.Bundle;
import com.github.egui_mobile.EguiNativeActivity;

public final class GalleryActivity extends EguiNativeActivity {
    private volatile GalleryController gallery;

    @Override protected void onCreate(Bundle state) {
        super.onCreate(state);
        gallery = new GalleryController(this);
    }

    public void galleryCommand(String json) {
        GalleryController controller = gallery;
        if (controller != null) controller.command(json);
    }

    public String galleryPoll() {
        GalleryController controller = gallery;
        return controller == null ? "{}" : controller.poll();
    }

    public void galleryAttachVideoSurface(android.view.Surface surface,int width,int height) {
        GalleryController controller=gallery;
        if(controller!=null) controller.attachVideoSurface(surface,width,height);
    }

    @Override protected void onPause() {
        if (gallery != null) gallery.pause();
        super.onPause();
    }

    @Override protected void onResume() {
        super.onResume();
        if (gallery != null) gallery.refreshAccess();
    }

    @Override protected void onDestroy() {
        if (gallery != null) gallery.close();
        super.onDestroy();
    }

    @Override public void onRequestPermissionsResult(int code, String[] permissions, int[] results) {
        super.onRequestPermissionsResult(code, permissions, results);
        if (gallery != null) gallery.scan();
    }

    @Override protected void onActivityResult(int request, int result, Intent data) {
        super.onActivityResult(request, result, data);
        if (gallery != null) gallery.activityResult(request, result, data);
    }
}
