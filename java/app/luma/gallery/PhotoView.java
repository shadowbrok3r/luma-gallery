package app.luma.gallery;

import android.content.Context;
import android.graphics.*;
import android.media.ExifInterface;
import android.util.LruCache;
import android.view.*;
import java.util.*;
import java.util.concurrent.*;

/** A bounded tile cache keeps one-to-one viewing independent of the source's megapixel count. */
final class PhotoView extends View {
    private BitmapRegionDecoder decoder;
    private Bitmap preview;
    private int imageWidth, imageHeight, rotation;
    private boolean mirror;
    private float scale = 1, fit = 1, panX, panY;
    private float touchStartX, touchStartY;
    private boolean swipeCandidate;
    private volatile long generation;
    private final Paint paint = new Paint(Paint.ANTI_ALIAS_FLAG | Paint.FILTER_BITMAP_FLAG);
    private final ExecutorService tiles = Executors.newSingleThreadExecutor();
    private final Set<String> pending = new HashSet<>();
    private final LruCache<String, Bitmap> cache = new LruCache<String, Bitmap>(24 * 1024 * 1024) {
        @Override protected int sizeOf(String key, Bitmap value) { return value.getAllocationByteCount(); }
    };
    private final ScaleGestureDetector pinch;
    private final GestureDetector gestures;
    private final GalleryController owner;

    PhotoView(Context context, GalleryController owner) {
        super(context);
        this.owner = owner;
        setBackgroundColor(Color.BLACK);
        setContentDescription("Photo viewer");
        pinch = new ScaleGestureDetector(context, new ScaleGestureDetector.SimpleOnScaleGestureListener() {
            @Override public boolean onScaleBegin(ScaleGestureDetector detector) {
                swipeCandidate = false;
                return true;
            }
            @Override public boolean onScale(ScaleGestureDetector detector) {
                float previous = scale;
                scale = Math.max(fit, Math.min(8, scale * detector.getScaleFactor()));
                float ratio = scale / previous;
                panX = (panX + getWidth()/2f - detector.getFocusX()) * ratio - getWidth()/2f + detector.getFocusX();
                panY = (panY + getHeight()/2f - detector.getFocusY()) * ratio - getHeight()/2f + detector.getFocusY();
                clampPan(); invalidate(); return true;
            }
        });
        gestures = new GestureDetector(context, new GestureDetector.SimpleOnGestureListener() {
            @Override public boolean onDown(android.view.MotionEvent e) { return true; }
            @Override public boolean onDoubleTap(android.view.MotionEvent e) {
                swipeCandidate = false;
                zoom(scale > fit * 1.05f ? 0 : 1);
                return true;
            }
            @Override public boolean onScroll(android.view.MotionEvent e, android.view.MotionEvent e2, float dx, float dy) {
                if (!swipeCandidate && !pinch.isInProgress()) { panX -= dx; panY -= dy; clampPan(); invalidate(); }
                return true;
            }
        });
    }

    void load(String path, boolean alreadyOriented) {
        load(path, null, null, alreadyOriented);
    }

    void load(java.io.FileDescriptor input, android.net.Uri uri) {
        load(null, input, uri, false);
    }

    private void load(String path, java.io.FileDescriptor input, android.net.Uri uri, boolean alreadyOriented) {
        final long token = ++generation;
        tiles.execute(() -> {
            BitmapRegionDecoder next = null;
            Bitmap low = null;
            try {
                next = input == null ? BitmapRegionDecoder.newInstance(path, false) : BitmapRegionDecoder.newInstance(input, false);
                if (next == null) throw new IllegalArgumentException("Image format cannot be decoded");
                int w = next.getWidth(), h = next.getHeight();
                BitmapFactory.Options options = new BitmapFactory.Options();
                options.inSampleSize = 1;
                while (Math.max(w,h) / options.inSampleSize > 1600) options.inSampleSize *= 2;
                low = next.decodeRegion(new Rect(0,0,w,h), options);
                int orientation = ExifInterface.ORIENTATION_NORMAL;
                if (!alreadyOriented) try { orientation = (input == null ? new ExifInterface(path) : new ExifInterface(input)).getAttributeInt(ExifInterface.TAG_ORIENTATION, 1); } catch (Exception ignored) { }
                final BitmapRegionDecoder ready = next;
                final Bitmap bitmap = low;
                final int exif = orientation;
                post(() -> {
                    if (token != generation) { ready.recycle(); if (bitmap != null) bitmap.recycle(); return; }
                    BitmapRegionDecoder old = decoder;
                    decoder = ready; preview = bitmap; imageWidth = w; imageHeight = h;
                    rotation = exif == 6 || exif == 5 ? 90 : exif == 8 || exif == 7 ? 270 : exif == 3 || exif == 4 ? 180 : 0;
                    mirror = exif == 2 || exif == 4 || exif == 5 || exif == 7;
                    cache.evictAll(); pending.clear(); fitImage();
                    if (old != null) tiles.execute(old::recycle);
                    setContentDescription("Full resolution photo " + w + " by " + h);
                    owner.photoReady(this,w,h);
                    invalidate();
                });
            } catch (Exception e) {
                if (next != null) next.recycle();
                if (low != null) low.recycle();
                // Region decoding excludes GIF/BMP and some platform-supported formats.
                // Keep the fallback bounded, oriented, and usable as a still-photo editor source.
                try {
                    int[] dimensions = new int[2];
                    ImageDecoder.Source src = uri == null ? ImageDecoder.createSource(new java.io.File(path))
                        : ImageDecoder.createSource(getContext().getContentResolver(), uri);
                    Bitmap fallback = ImageDecoder.decodeBitmap(src, (d, info, unused) -> {
                        int w=info.getSize().getWidth(), h=info.getSize().getHeight();
                        dimensions[0]=w; dimensions[1]=h;
                        double factor=Math.min(1.0,3200.0/Math.max(w,h));
                        d.setTargetSize(Math.max(1,(int)(w*factor)),Math.max(1,(int)(h*factor)));
                        d.setAllocator(ImageDecoder.ALLOCATOR_SOFTWARE);
                    });
                    post(() -> {
                        if(token!=generation){fallback.recycle();return;}
                        BitmapRegionDecoder old=decoder; decoder=null; preview=fallback;
                        imageWidth=fallback.getWidth(); imageHeight=fallback.getHeight(); rotation=0; mirror=false;
                        cache.evictAll();pending.clear();fitImage();
                        if(old!=null)tiles.execute(old::recycle);
                        owner.photoPreviewReady(this,dimensions[0],dimensions[1]);invalidate();
                    });
                } catch(Exception | OutOfMemoryError failed) {
                    if (token == generation) owner.error("Photo: " + failed.getMessage());
                }
            }
        });
    }

    void fitImage() {
        if (imageWidth <= 0 || imageHeight <= 0) { scale = fit = 1; return; }
        int w = rotation % 180 == 0 ? imageWidth : imageHeight;
        int h = rotation % 180 == 0 ? imageHeight : imageWidth;
        fit = Math.min(getWidth() / (float)Math.max(1,w), getHeight() / (float)Math.max(1,h));
        scale = fit; panX = panY = 0;
    }
    void zoom(float value) { scale = value <= 0 ? fit : Math.max(fit, value); panX = panY = 0; invalidate(); }
    float zoomValue() { return scale; }
    @Override protected void onSizeChanged(int w,int h,int oldW,int oldH) { fitImage(); }
    void clampPan() {
        float w = (rotation % 180 == 0 ? imageWidth : imageHeight) * scale;
        float h = (rotation % 180 == 0 ? imageHeight : imageWidth) * scale;
        float maxX = Math.max(0, (w - getWidth())/2), maxY = Math.max(0,(h-getHeight())/2);
        panX = Math.max(-maxX, Math.min(maxX, panX)); panY = Math.max(-maxY, Math.min(maxY, panY));
    }
    @Override public boolean onTouchEvent(android.view.MotionEvent event) {
        int action = event.getActionMasked();
        if (action == MotionEvent.ACTION_DOWN) {
            touchStartX = event.getX(); touchStartY = event.getY();
            swipeCandidate = preview != null && scale <= fit * 1.02f;
        }
        // A pinch never turns into navigation when the second finger is lifted.
        if (event.getPointerCount() > 1 || action == MotionEvent.ACTION_CANCEL) swipeCandidate = false;
        pinch.onTouchEvent(event); gestures.onTouchEvent(event);
        if (action == MotionEvent.ACTION_UP) {
            float dx = event.getX() - touchStartX, dy = event.getY() - touchStartY;
            float density = getResources().getDisplayMetrics().density;
            float threshold = Math.max(48 * density, Math.min(getWidth() * 0.18f, 120 * density));
            if (swipeCandidate && Math.abs(dx) >= threshold && Math.abs(dx) > Math.abs(dy) * 1.5f) {
                owner.navigatePhoto(this, dx < 0 ? 1 : -1);
            }
            swipeCandidate = false;
        }
        return true;
    }
    @Override protected void onDraw(Canvas canvas) {
        super.onDraw(canvas);
        if (preview == null || scale <= 0) return;
        Matrix transform = new Matrix();
        transform.postTranslate(-imageWidth/2f, -imageHeight/2f);
        transform.postScale(mirror ? -1 : 1, 1);
        transform.postRotate(rotation);
        transform.postScale(scale, scale);
        transform.postTranslate(getWidth()/2f + panX, getHeight()/2f + panY);
        Matrix inverse = new Matrix(); transform.invert(inverse);
        RectF visible = new RectF(0,0,getWidth(),getHeight()); inverse.mapRect(visible);
        visible.intersect(0,0,imageWidth,imageHeight);
        canvas.save(); canvas.concat(transform);
        canvas.drawBitmap(preview, null, new Rect(0,0,imageWidth,imageHeight), paint);
        if(decoder==null){canvas.restore();return;}
        int sample = 1;
        while (sample * 2 * scale <= 1) sample *= 2;
        int edge = 512 * sample;
        final long token = generation;
        final BitmapRegionDecoder current = decoder;
        for (int y = Math.max(0,(int)visible.top/edge*edge); y < Math.min(imageHeight,visible.bottom); y += edge) {
            for (int x = Math.max(0,(int)visible.left/edge*edge); x < Math.min(imageWidth,visible.right); x += edge) {
                Rect region = new Rect(x,y,Math.min(x+edge,imageWidth),Math.min(y+edge,imageHeight));
                String key = token + ":" + sample + ":" + x + ":" + y;
                Bitmap tile = cache.get(key);
                if (tile != null) canvas.drawBitmap(tile, null, region, paint);
                else if (pending.size() < 32 && pending.add(key)) {
                    final int sampling = sample;
                    tiles.execute(() -> {
                        Bitmap decoded = null;
                        try {
                            if (token == generation && !current.isRecycled()) {
                                BitmapFactory.Options options = new BitmapFactory.Options(); options.inSampleSize = sampling;
                                decoded = current.decodeRegion(region, options);
                            }
                        } catch (Exception ignored) { }
                        final Bitmap ready = decoded;
                        post(() -> { pending.remove(key); if (token == generation && ready != null) cache.put(key, ready); invalidate(); });
                    });
                }
            }
        }
        canvas.restore();
    }
    void close() {
        generation++;
        BitmapRegionDecoder old = decoder; decoder = null; preview = null; cache.evictAll();
        if (old != null) tiles.execute(old::recycle);
        tiles.shutdown();
    }
}
