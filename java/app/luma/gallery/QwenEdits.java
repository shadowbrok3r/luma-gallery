package app.luma.gallery;

import android.content.SharedPreferences;
import org.json.*;
import java.io.*;
import java.net.*;
import java.nio.charset.StandardCharsets;
import java.util.*;
import java.util.concurrent.*;
import java.util.function.Consumer;

/** The same alpha-PNG / comfy-gate edit protocol used by comfyui-android. */
final class QwenEdits {
    private static final String DEFAULT_SERVER="https://comfy.shadowbroker.app";
    private final PhotoEdits photos;
    private final Consumer<JSONObject> emit;
    private final SharedPreferences prefs;
    private final ExecutorService worker=Executors.newSingleThreadExecutor();
    private volatile boolean stopped, closed;
    volatile boolean busy;
    volatile String status="";
    private volatile HttpURLConnection connection;

    QwenEdits(PhotoEdits photos, Consumer<JSONObject> emit) {
        this.photos=photos; this.emit=emit;
        prefs=photos.activity.getSharedPreferences("qwen-editor",0);
        if(!prefs.contains("url"))prefs.edit().putString("url",DEFAULT_SERVER).apply();
    }

    private void event(JSONObject request,String type,JSONObject fields) {
        try { fields.put("type",type).put("session",request.optString("session")); if(!closed)emit.accept(fields); }
        catch(Exception ignored){}
    }
    private void fail(JSONObject request,Exception error) {
        try { event(request,"qwen_error",new JSONObject().put("pending",!prefs.getString("pending","").isEmpty()).put("message",stopped?"Stopped waiting. Resume to collect this job; it may still be running on the server.":error.getMessage())); }
        catch(Exception ignored){}
    }

    void config(JSONObject request) {
        if(busy)return;
        stopped=false;
        worker.execute(() -> {
            try {
                if(request.has("url")) {
                    String base=normalize(request.getString("url"));
                    SharedPreferences.Editor editor=prefs.edit();
                    if(!base.equals(prefs.getString("url",""))) {
                        editor.remove("key").remove("cookie").remove("pending").remove("result");
                    }
                    editor.putString("url",base);
                    if(request.optBoolean("clear_auth"))editor.remove("key").remove("cookie");
                    if(request.has("key"))editor.putString("key",credential(request.getString("key")));
                    editor.commit();
                }
                if(request.has("password"))login(request.getString("username"),request.getString("password"));
                if(request.optBoolean("check")) {
                    JSONObject nodes=json("GET","/object_info/TextEncodeQwenImage21",null);
                    if(!nodes.has("TextEncodeQwenImage21"))throw new IOException("This server does not offer Qwen Image Edit 2.1. Use the same updated comfy-gate server as ComfyUI Android.");
                }
                event(request,"qwen_config",new JSONObject().put("url",prefs.getString("url",""))
                    .put("authenticated",!prefs.getString("key","").isEmpty()||!prefs.getString("cookie","").isEmpty())
                    .put("pending",(!prefs.getString("pending","").isEmpty()||new File(prefs.getString("result","")).isFile())&&request.optString("uri").equals(prefs.getString("pending_uri","")))
                    .put("message",request.optBoolean("check")?"Qwen model is available":request.has("password")?"Signed in":""));
            }catch(Exception e){fail(request,e);}
        });
    }

    static String normalize(String value) throws Exception {
        URI uri=new URI(value.trim());
        if(!("http".equalsIgnoreCase(uri.getScheme())||"https".equalsIgnoreCase(uri.getScheme())) || uri.getHost()==null
            || uri.getUserInfo()!=null || uri.getQuery()!=null || uri.getFragment()!=null)
            throw new IOException("Enter the server's http:// or https:// address");
        return uri.toString().replaceAll("/+$","");
    }
    private static String credential(String value) throws IOException {
        value=value.trim();
        for(char c:value.toCharArray())if(c<' '||c==127)throw new IOException("The credential contains a line break or control character");
        return value;
    }
    private HttpURLConnection open(String method,String path) throws Exception {
        if(closed||stopped)throw new InterruptedIOException("Stopped waiting");
        String base=normalize(prefs.getString("url",""));
        HttpURLConnection c=(HttpURLConnection)new URL(base+path).openConnection();
        c.setInstanceFollowRedirects(false); // Never forward a credential to a redirected host.
        c.setConnectTimeout(20000); c.setReadTimeout(120000); c.setRequestMethod(method);
        c.setRequestProperty("Accept","application/json");
        c.setRequestProperty("User-Agent","LumaGallery/0.1.4 Android");
        String key=prefs.getString("key",""), cookie=prefs.getString("cookie","");
        if(!key.isEmpty()){c.setRequestProperty("X-Api-Key",key);c.setRequestProperty("Authorization","Bearer "+key);}
        if(!cookie.isEmpty())c.setRequestProperty("Cookie","cg_session="+cookie);
        connection=c; return c;
    }
    private static byte[] read(InputStream stream,int limit) throws IOException {
        if(stream==null)return new byte[0];
        try(InputStream in=stream; ByteArrayOutputStream bytes=new ByteArrayOutputStream()) {
            byte[] buffer=new byte[16384]; int n;
            while((n=in.read(buffer))!=-1) { if(bytes.size()+n>limit)throw new IOException("Server response is too large"); bytes.write(buffer,0,n); }
            return bytes.toByteArray();
        }
    }
    private byte[] response(HttpURLConnection c,int limit) throws Exception {
        int code=c.getResponseCode();
        if(code>=200&&code<300)return read(c.getInputStream(),limit);
        if(code==401||code==403)throw new IOException("Server access refused. Check your API key or sign in again.");
        if(code==404||code==405)throw new IOException("Qwen edit route is unavailable. Check the server address and comfy-gate version.");
        if(code==422)throw new IOException("The server found no painted area. Paint a mask and try again.");
        if(code>=300&&code<400)throw new IOException("The server redirected this request. Enter its final address and sign in.");
        throw new IOException("Server request failed (HTTP "+code+")");
    }
    private JSONObject json(String method,String path,JSONObject body) throws Exception {
        HttpURLConnection c=open(method,path);
        try {
            if(body!=null) {
                byte[] bytes=body.toString().getBytes(StandardCharsets.UTF_8); c.setDoOutput(true);
                c.setRequestProperty("Content-Type","application/json");c.setFixedLengthStreamingMode(bytes.length);
                try(OutputStream out=c.getOutputStream()){out.write(bytes);}
            }
            return new JSONObject(new String(response(c,4*1024*1024),StandardCharsets.UTF_8));
        }finally{c.disconnect();connection=null;}
    }
    private static String enc(String value) throws Exception {return URLEncoder.encode(value,"UTF-8");}

    private void login(String username,String password) throws Exception {
        stopped=false;
        HttpURLConnection c=open("POST","/login");
        try {
            byte[] form=("username="+enc(username)+"&password="+enc(password)).getBytes(StandardCharsets.UTF_8);
            c.setDoOutput(true);c.setRequestProperty("Content-Type","application/x-www-form-urlencoded");c.setFixedLengthStreamingMode(form.length);
            try(OutputStream out=c.getOutputStream()){out.write(form);}
            int code=c.getResponseCode();
            for(Map.Entry<String,List<String>> h:c.getHeaderFields().entrySet()) {
                if(!"Set-Cookie".equalsIgnoreCase(h.getKey()))continue;
                for(String cookie:h.getValue()) {
                    String pair=cookie.split(";",2)[0];
                    if(pair.startsWith("cg_session=")&&pair.length()>11){prefs.edit().putString("cookie",credential(pair.substring(11))).commit();return;}
                }
            }
            throw new IOException(code==429?"Too many sign-in attempts. Try again later.":"Sign-in failed. Check username and password.");
        } finally{c.disconnect();connection=null;}
    }

    private String upload(File input) throws Exception {
        String boundary="luma-"+UUID.randomUUID(),name="luma_edit_"+UUID.randomUUID()+".png";
        byte[] before=("--"+boundary+"\r\nContent-Disposition: form-data; name=\"image\"; filename=\""+name+"\"\r\nContent-Type: image/png\r\n\r\n").getBytes(StandardCharsets.UTF_8);
        byte[] after=("\r\n--"+boundary+"\r\nContent-Disposition: form-data; name=\"type\"\r\n\r\ninput\r\n--"+boundary+"--\r\n").getBytes(StandardCharsets.UTF_8);
        HttpURLConnection c=open("POST","/upload/image");
        try {
            c.setDoOutput(true);c.setRequestProperty("Content-Type","multipart/form-data; boundary="+boundary);
            c.setFixedLengthStreamingMode(before.length+input.length()+after.length);
            try(OutputStream out=c.getOutputStream();InputStream in=new FileInputStream(input)) {
                out.write(before);byte[] buffer=new byte[65536];int n;
                while((n=in.read(buffer))!=-1){if(stopped)throw new InterruptedIOException();out.write(buffer,0,n);}out.write(after);
            }
            JSONObject result=new JSONObject(new String(response(c,65536),StandardCharsets.UTF_8));
            // comfy-gate returns its own user-tagged bare filename; do not substitute our name.
            return result.getString("name");
        }finally{c.disconnect();connection=null;}
    }

    void run(JSONObject request,boolean resume) {
        if(busy)return;
        busy=true; stopped=false; status=resume?"Resuming Qwen edit":"Uploading painted photo";
        worker.execute(() -> {
            try {
                String id;
                if(resume) {
                    id=prefs.getString("pending","");
                    if(!request.optString("uri").equals(prefs.getString("pending_uri","")))throw new IOException("Reopen the original photo to resume its Qwen edit");
                    if(id.isEmpty()) {
                        File result=photos.local(prefs.getString("result",""));
                        event(request,"qwen_result",new JSONObject().put("path",result.getAbsolutePath()).put("uri",request.optString("uri")));return;
                    }
                }else {
                    if(request.optString("instruction").trim().isEmpty())throw new IOException("Describe what should change");
                    String name=upload(photos.local(request.getString("path")));
                    status="Queueing Qwen edit";
                    JSONObject body=new JSONObject().put("image",name).put("instruction",request.getString("instruction").trim())
                        .put("steps",Math.max(1,Math.min(60,request.optInt("steps",25)))).put("turbo",request.optBoolean("turbo"))
                        .put("seed",new java.security.SecureRandom().nextInt(Integer.MAX_VALUE)).put("client_id",UUID.randomUUID().toString());
                    JSONObject queued=json("POST","/comfyui-android/edit",body);
                    if(queued.optJSONObject("node_errors")!=null&&queued.getJSONObject("node_errors").length()>0)
                        throw new IOException("The server refused part of the Qwen workflow. Check its model setup.");
                    id=queued.getString("prompt_id");
                    prefs.edit().putString("pending",id).putString("pending_uri",request.optString("uri")).remove("result").commit();
                }
                status="Qwen queued / generating";
                long deadline=System.nanoTime()+TimeUnit.HOURS.toNanos(1);
                while(!stopped&&!closed&&System.nanoTime()<deadline) {
                    JSONObject history=json("GET","/history/"+enc(id),null).optJSONObject(id);
                    if(history!=null) {
                        JSONObject state=history.optJSONObject("status");
                        if(state!=null&&"error".equals(state.optString("status_str")))throw new IOException("Qwen failed on the server. Check the workflow and installed model.");
                        if(state!=null&&state.optBoolean("completed")) {
                            JSONObject outputs=history.optJSONObject("outputs"),image=null;
                            if(outputs!=null)for(Iterator<String> keys=outputs.keys();keys.hasNext();) {
                                JSONObject node=outputs.optJSONObject(keys.next());
                                JSONArray images=node==null?null:node.optJSONArray("images");
                                if(images!=null)for(int i=0;i<images.length();i++) {
                                    JSONObject candidate=images.getJSONObject(i);
                                    if("output".equals(candidate.optString("type","output")))image=candidate;
                                }
                            }
                            if(image==null)throw new IOException("Qwen completed without an output image");
                            status="Downloading Qwen result";
                            String query="/view?filename="+enc(image.getString("filename"))+"&subfolder="+enc(image.optString("subfolder"))+"&type="+enc(image.optString("type","output"));
                            HttpURLConnection c=open("GET",query);File output=new File(photos.directory,UUID.randomUUID()+"-qwen.png");
                            try {
                                byte[] bytes=response(c,64*1024*1024);
                                try(OutputStream out=new FileOutputStream(output)){out.write(bytes);}
                            }finally{c.disconnect();connection=null;}
                            prefs.edit().remove("pending").putString("result",output.getAbsolutePath()).commit();
                            event(request,"qwen_result",new JSONObject().put("path",output.getAbsolutePath()).put("uri",prefs.getString("pending_uri","")));
                            return;
                        }
                    }
                    Thread.sleep(2000);
                }
                throw new IOException("Still waiting for Qwen. Resume this job when the server is ready.");
            }catch(Exception e){fail(request,e);}
            finally{busy=false;status="";stopped=false;}
        });
    }
    void stop(){stopped=true;HttpURLConnection c=connection;if(c!=null)c.disconnect();}
    void close(){closed=true;stop();worker.shutdownNow();}
}
