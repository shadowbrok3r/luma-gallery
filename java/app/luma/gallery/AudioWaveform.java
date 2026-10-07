package app.luma.gallery;

import org.json.*;
import java.util.*;
import java.util.concurrent.*;
import java.util.concurrent.atomic.*;
import java.util.function.Consumer;

/** Bounded peak analysis of the original audio, independent of the playback proxy. */
final class AudioWaveform {
    private final MediaFiles files;
    private final Consumer<JSONObject> emit;
    private final ExecutorService worker=Executors.newSingleThreadExecutor();
    private final AtomicReference<Process> process=new AtomicReference<>();
    private final AtomicLong generation=new AtomicLong();
    AudioWaveform(MediaFiles files,Consumer<JSONObject> emit){this.files=files;this.emit=emit;}
    void cancel(){generation.incrementAndGet();Process p=process.get();if(p!=null)p.destroyForcibly();}
    void read(JSONObject item) {
        cancel();long token=generation.get();
        worker.execute(() -> {
            try {
                if(token!=generation.get())return;
                JSONObject metadata=files.probe(item);JSONArray streams=metadata.optJSONArray("streams");boolean audio=false;
                if(streams!=null)for(int i=0;i<streams.length();i++)if("audio".equals(streams.getJSONObject(i).optString("codec_type")))audio=true;
                if(!audio){send(item,token,new JSONArray(),0,"No audio track");return;}
                long duration=Math.max(item.optLong("duration"),(long)(metadata.getJSONObject("format").optDouble("duration",0)*1000));
                int samples=Math.max(160,(int)Math.ceil(duration*8.0/24000));
                double step=samples/8.0;
                float[] peaks=new float[24002];int[] index={0},length={0};
                try(MediaFiles.Source source=files.source(item.getString("uri"))) {
                    List<String> args=Arrays.asList("-v","error","-nostdin","-threads","2","-copyts","-start_at_zero","-i",source.path,
                        "-map","0:a:0","-vn","-af","aresample=8000:async=1:first_pts=0,asetnsamples=n="+samples+":p=1,astats=metadata=1:reset=1:measure_perchannel=none:measure_overall=Peak_level,ametadata=print:key=lavfi.astats.Overall.Peak_level:file=-",
                        "-c:a","pcm_s16le","-f","null","-");
                    files.run("ffmpeg",args,1800,process,line -> {
                        if(token!=generation.get()){Process p=process.get();if(p!=null)p.destroyForcibly();return;}
                        try {
                            if(line.contains("pts_time:"))index[0]=(int)Math.round(Double.parseDouble(line.substring(line.indexOf("pts_time:")+9).trim())*1000/step);
                            if(line.startsWith("lavfi.astats.Overall.Peak_level=")) {
                                float level=line.endsWith("-inf")?0:(float)Math.pow(10,Double.parseDouble(line.substring(line.indexOf('=')+1))/20);
                                if(index[0]>=0&&index[0]<peaks.length){peaks[index[0]]=Math.min(1,Math.max(0,level));length[0]=Math.max(length[0],index[0]+1);}
                            }
                        }catch(NumberFormatException ignored){}
                    });
                }
                JSONArray result=new JSONArray();for(int i=0;i<length[0];i++)result.put(peaks[i]);
                send(item,token,result,step,length[0]==0?"Audio could not be analyzed":"");
            }catch(Exception e){send(item,token,new JSONArray(),0,"Waveform unavailable. Tap retry.");}
        });
    }
    private void send(JSONObject item,long token,JSONArray peaks,double step,String status) {
        if(token!=generation.get())return;
        try{emit.accept(new JSONObject().put("type","waveform").put("uri",item.optString("uri")).put("peaks",peaks).put("step_ms",step).put("status",status));}catch(Exception ignored){}
    }
    void close(){cancel();worker.shutdownNow();}
}
