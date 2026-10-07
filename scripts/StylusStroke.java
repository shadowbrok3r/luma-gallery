import android.os.SystemClock;
import android.view.InputDevice;
import android.view.InputEvent;
import android.view.MotionEvent;

/** Shell-only emulator probe: one pressure-controlled contact, with no MOVE while held.
 * Compile with the Android SDK and run through app_process, never package in the app.
 */
public final class StylusStroke {
    public static void main(String[] args) throws Exception {
        float x=Float.parseFloat(args[0]), y=Float.parseFloat(args[1]);
        float pressure=Float.parseFloat(args[2]);
        int tool=Integer.parseInt(args[3]), buttons=Integer.parseInt(args[4]);
        long hold=Long.parseLong(args[5]);
        Class<?> type;
        try {type=Class.forName("android.hardware.input.InputManagerGlobal");}
        catch(ClassNotFoundException older){type=Class.forName("android.hardware.input.InputManager");}
        Object manager=type.getMethod("getInstance").invoke(null);
        java.lang.reflect.Method inject=type.getMethod("injectInputEvent",InputEvent.class,int.class);
        MotionEvent.PointerProperties props=new MotionEvent.PointerProperties();
        props.id=0;props.toolType=tool;
        MotionEvent.PointerCoords coords=new MotionEvent.PointerCoords();
        coords.x=x;coords.y=y;coords.pressure=pressure;coords.size=.01f;
        long down=SystemClock.uptimeMillis();
        try {
            send(inject,manager,down,MotionEvent.ACTION_DOWN,props,coords,buttons);
            SystemClock.sleep(hold);
        } finally {
            coords.pressure=0;
            send(inject,manager,down,MotionEvent.ACTION_UP,props,coords,0);
        }
    }
    private static void send(java.lang.reflect.Method inject,Object manager,long down,int action,
                             MotionEvent.PointerProperties props,MotionEvent.PointerCoords coords,int buttons) throws Exception {
        MotionEvent event=MotionEvent.obtain(down,SystemClock.uptimeMillis(),action,1,
            new MotionEvent.PointerProperties[]{props},new MotionEvent.PointerCoords[]{coords},
            0,buttons,1,1,0,0,InputDevice.SOURCE_STYLUS,0);
        try {if(!Boolean.TRUE.equals(inject.invoke(manager,event,2)))throw new IllegalStateException("Input rejected");}
        finally {event.recycle();}
    }
}
