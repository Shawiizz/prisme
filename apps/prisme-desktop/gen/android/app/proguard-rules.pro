# The methods the native side calls (usb_android.rs, input_access.rs, commands/apk.rs,
# lock_player.rs): nothing calls them from Kotlin, and R8 would take them out of a release build
# as unused.
-keep class com.shawiizz.prisme.UsbAudio {
    public static java.lang.String[] cards();
    public static boolean allowed(java.lang.String);
    public static void ask(java.lang.String);
    public static int open(java.lang.String);
    public static byte[] descriptors(java.lang.String);
    public static void close(java.lang.String);
}
-keep class com.shawiizz.prisme.MainActivity {
    public static void askForTheInputs();
    public static void showPlayer(android.content.Context, java.lang.String);
    public static void showCover(android.content.Context, java.lang.String, byte[]);
}
-keep class com.shawiizz.prisme.Update {
    public static int install(java.lang.String);
    public static void allow();
}
