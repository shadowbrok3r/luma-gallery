package app.luma.gallery;

import android.content.*;
import android.graphics.*;
import android.net.Uri;
import android.provider.MediaStore;
import org.json.*;
import java.io.*;
import java.util.UUID;
import java.util.concurrent.*;
import java.util.concurrent.atomic.AtomicReference;
import java.util.function.Consumer;

/** Oriented previews and copy-only exports for every format Android's ImageDecoder can read. */
final class PhotoEdits {
    static final long MAX_PIXELS = 32_000_000;
    final GalleryActivity activity;
    final MediaFiles files;
    final File directory;
    private final Consumer<JSONObject> emit;
    private final Runnable scan;
    private final ExecutorService worker = Executors.newSingleThreadExecutor();
    private final AtomicReference<Process> process = new AtomicReference<>();
    volatile boolean busy, closed;
    volatile String status = "";

    PhotoEdits(GalleryActivity activity, MediaFiles files, Consumer<JSONObject> emit, Runnable scan) {
        this.activity = activity; this.files = files; this.emit = emit; this.scan = scan;
        directory = new File(activity.getCacheDir(), "editor"); directory.mkdirs();
        // Completed Qwen results stay available for Resume; stale transient work does not accumulate.
        String retained=activity.getSharedPreferences("qwen-editor",0).getString("result","");
        File[] cached=directory.listFiles();
        long cutoff=System.currentTimeMillis()-TimeUnit.DAYS.toMillis(1);
        if(cached!=null)for(File file:cached)
            if(file.isFile()&&file.lastModified()<cutoff&&!file.getAbsolutePath().equals(retained))file.delete();
    }

    private void event(JSONObject request, String type, JSONObject fields) {
        try { fields.put("type", type).put("session", request.optString("session")); if (!closed) emit.accept(fields); }
        catch (Exception ignored) { }
    }

    private ImageDecoder.Source source(JSONObject item) throws Exception {
        if ("raw".equals(item.optString("kind")))
            return ImageDecoder.createSource(files.developRaw(item, process));
        return ImageDecoder.createSource(activity.getContentResolver(), Uri.parse(item.getString("uri")));
    }

    /** ImageDecoder applies all EXIF orientations and converts wide-gamut sources into SDR sRGB. */
    private Bitmap decode(ImageDecoder.Source source, int maxSide, int[] dimensions) throws IOException {
        return ImageDecoder.decodeBitmap(source, (decoder, info, ignored) -> {
            int w = info.getSize().getWidth(), h = info.getSize().getHeight();
            if (dimensions != null) { dimensions[0] = w; dimensions[1] = h; }
            double scale = Math.min(1.0, Math.sqrt(MAX_PIXELS / (double)((long)w*h)));
            if (maxSide > 0) scale = Math.min(scale, maxSide / (double)Math.max(w,h));
            decoder.setTargetSize(Math.max(1,(int)Math.round(w*scale)), Math.max(1,(int)Math.round(h*scale)));
            decoder.setAllocator(ImageDecoder.ALLOCATOR_SOFTWARE);
            decoder.setTargetColorSpace(ColorSpace.get(ColorSpace.Named.SRGB));
        });
    }

    void prepare(JSONObject request) {
        worker.execute(() -> {
            Bitmap bitmap = null;
            try {
                int[] dimensions = new int[2];
                bitmap = decode(source(request.getJSONObject("item")), 1600, dimensions);
                File preview = new File(directory, UUID.randomUUID()+".png");
                write(bitmap, preview, true);
                double scale = Math.min(1.0, Math.sqrt(MAX_PIXELS / (double)((long)dimensions[0]*dimensions[1])));
                event(request, "edit_ready", new JSONObject().put("path",preview.getAbsolutePath())
                    .put("width",dimensions[0]).put("height",dimensions[1])
                    .put("export_width",Math.max(1,Math.round(dimensions[0]*scale)))
                    .put("export_height",Math.max(1,Math.round(dimensions[1]*scale))));
            } catch (Exception | OutOfMemoryError e) { failure(request, e); }
            finally { if (bitmap != null) bitmap.recycle(); }
        });
    }

    static ColorMatrix colorMatrix(JSONObject recipe) {
        ColorMatrix matrix = new ColorMatrix();
        matrix.setSaturation((float)recipe.optDouble("saturation",1));
        float contrast = (float)recipe.optDouble("contrast",1);
        float gain = (float)Math.pow(2, recipe.optDouble("exposure",0))*contrast;
        float offset = 127.5f*(1-contrast);
        matrix.postConcat(new ColorMatrix(new float[]{gain,0,0,0,offset, 0,gain,0,0,offset,
            0,0,gain,0,offset, 0,0,0,1,0}));
        return matrix;
    }

    /** Crop coordinates are in the rotated/flipped, EXIF-oriented image. */
    static Bitmap render(Bitmap input, JSONObject recipe) throws Exception {
        int rotation = Math.floorMod(recipe.optInt("rotation"),4);
        int width = rotation%2 == 0 ? input.getWidth() : input.getHeight();
        int height = rotation%2 == 0 ? input.getHeight() : input.getWidth();
        JSONObject crop = recipe.optJSONObject("crop");
        double left = crop == null ? 0 : crop.optDouble("left",0), top = crop == null ? 0 : crop.optDouble("top",0);
        double right = crop == null ? 1 : crop.optDouble("right",1), bottom = crop == null ? 1 : crop.optDouble("bottom",1);
        if (!Double.isFinite(left+top+right+bottom) || left<0 || top<0 || right>1 || bottom>1 || left>=right || top>=bottom)
            throw new IOException("Crop is outside the photo");
        int x = Math.min(width-1,(int)Math.round(left*width)), y = Math.min(height-1,(int)Math.round(top*height));
        int w = Math.max(1,Math.min(width,(int)Math.round(right*width))-x), h = Math.max(1,Math.min(height,(int)Math.round(bottom*height))-y);
        Bitmap output = Bitmap.createBitmap(w,h,Bitmap.Config.ARGB_8888);
        Matrix transform = new Matrix();
        transform.setRotate(rotation*90);
        RectF bounds = new RectF(0,0,input.getWidth(),input.getHeight()); transform.mapRect(bounds);
        transform.postTranslate(-bounds.left,-bounds.top);
        if (recipe.optBoolean("flip")) transform.postScale(-1,1,width/2f,height/2f);
        transform.postTranslate(-x,-y);
        Paint paint = new Paint(Paint.ANTI_ALIAS_FLAG | Paint.FILTER_BITMAP_FLAG);
        paint.setColorFilter(new ColorMatrixColorFilter(colorMatrix(recipe)));
        new Canvas(output).drawBitmap(input,transform,paint);
        return output;
    }

    void save(JSONObject request) {
        if (busy) return;
        busy=true; status="Saving photo";
        worker.execute(() -> {
            Bitmap original=null, result=null; File output=null;
            try {
                boolean ai=request.has("result");
                if (ai) original=decode(ImageDecoder.createSource(local(request.getString("result"))),0,null);
                else original=decode(source(request.getJSONObject("item")),0,null);
                result=ai ? original : render(original,request.getJSONObject("recipe"));
                boolean png=request.optBoolean("png",ai);
                output=new File(directory,UUID.randomUUID()+(png?".png":".jpg"));
                write(result,output,png);
                String name=request.getJSONObject("item").optString("name","photo").replaceFirst("\\.[^.]+$","")
                    +(ai?"_qwen_":"_edit_")+System.currentTimeMillis()+(png?".png":".jpg");
                Uri uri=publish(output,name,png?"image/png":"image/jpeg");
                event(request,"photo_saved",new JSONObject().put("uri",uri.toString()).put("width",result.getWidth()).put("height",result.getHeight()));
                scan.run();
            } catch (Exception | OutOfMemoryError e) { failure(request,e); }
            finally {
                if(result!=null && result!=original) result.recycle();
                if(original!=null) original.recycle(); if(output!=null) output.delete(); busy=false; status="";
            }
        });
    }

    /** Only work files created inside this editor can be supplied across the Rust bridge. */
    File local(String path) throws IOException {
        File file=new File(path).getCanonicalFile();
        if (!file.getPath().startsWith(directory.getCanonicalPath()+File.separator) || !file.isFile())
            throw new IOException("Editor file is no longer available. Reopen the photo.");
        return file;
    }

    void external(JSONObject request) {
        if (busy) return;
        busy=true; status="Preparing editor copy";
        worker.execute(() -> {
            Bitmap original=null; File output=null;
            try {
                JSONObject item=request.getJSONObject("item");
                original=decode(source(item),0,null);
                output=new File(directory,UUID.randomUUID()+".jpg"); write(original,output,false);
                Uri uri=publish(output,"Luma_external_"+System.currentTimeMillis()+".jpg","image/jpeg");
                Intent edit=new Intent(Intent.ACTION_EDIT).setDataAndType(uri,"image/jpeg")
                    .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION|Intent.FLAG_GRANT_WRITE_URI_PERMISSION);
                edit.setClipData(ClipData.newRawUri("Photo copy",uri));
                activity.runOnUiThread(() -> {
                    try { activity.startActivityForResult(Intent.createChooser(edit,"Edit a copy"),703); }
                    catch(ActivityNotFoundException e) { failure(request,new IOException("No photo editor is installed")); }
                });
                event(request,"external_copy",new JSONObject().put("uri",uri.toString())); scan.run();
            } catch(Exception | OutOfMemoryError e) { failure(request,e); }
            finally { if(original!=null)original.recycle(); if(output!=null)output.delete(); busy=false; status=""; }
        });
    }

    private static void write(Bitmap bitmap,File target,boolean png) throws IOException {
        try(OutputStream out=new FileOutputStream(target)) {
            if(!bitmap.compress(png?Bitmap.CompressFormat.PNG:Bitmap.CompressFormat.JPEG,95,out)) throw new IOException("Photo encoding failed");
        }
    }

    private Uri publish(File file,String name,String mime) throws Exception {
        ContentResolver resolver=activity.getContentResolver();
        ContentValues values=new ContentValues();
        values.put(MediaStore.MediaColumns.DISPLAY_NAME,name); values.put(MediaStore.MediaColumns.MIME_TYPE,mime);
        values.put(MediaStore.MediaColumns.RELATIVE_PATH,"Pictures/Luma"); values.put(MediaStore.MediaColumns.IS_PENDING,1);
        Uri uri=resolver.insert(MediaStore.Images.Media.EXTERNAL_CONTENT_URI,values);
        if(uri==null)throw new IOException("Could not create photo copy");
        try {
            try(InputStream in=new FileInputStream(file); OutputStream out=resolver.openOutputStream(uri)) {
                if(out==null)throw new IOException("Photo destination is unavailable");
                byte[] buffer=new byte[65536]; int n;
                while((n=in.read(buffer))!=-1) { if(closed)throw new IOException("Editor closed"); out.write(buffer,0,n); }
            }
            values.clear(); values.put(MediaStore.MediaColumns.IS_PENDING,0);
            resolver.update(uri,values,null,null); return uri;
        } catch(Exception e) { resolver.delete(uri,null,null); throw e; }
    }

    private void failure(JSONObject request,Throwable error) {
        String message=error instanceof OutOfMemoryError?"Not enough memory to edit this photo. Close other apps and try again.":String.valueOf(error.getMessage());
        try { event(request,"edit_error",new JSONObject().put("message",message)); }catch(Exception ignored){}
    }
    void close() { closed=true; worker.shutdownNow(); Process p=process.get(); if(p!=null)p.destroyForcibly(); }
}
