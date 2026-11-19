# The methods the native side calls (usb_android.rs, input_access.rs): nothing calls them from
# Kotlin, and R8 would take them out of a release build as unused.
-keep class com.shawiizz.prisme.UsbAudio {
    public static java.lang.String[] cards();
    public static int open(java.lang.String);
    public static byte[] descriptors(java.lang.String);
    public static void close(java.lang.String);
}
-keep class com.shawiizz.prisme.MainActivity {
    public static void askForTheInputs();
}
