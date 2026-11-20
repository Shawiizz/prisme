package com.shawiizz.prisme

import android.Manifest
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.content.res.Configuration
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Color
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.view.View
import android.view.WindowManager
import android.webkit.WebView
import androidx.activity.OnBackPressedCallback
import androidx.activity.SystemBarStyle
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.ActivityResultLauncher
import androidx.activity.result.contract.ActivityResultContracts.RequestMultiplePermissions
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat

/** How long the screen stays on once every deck stopped. */
private const val SCREEN_LINGER_MS = 60 * 1000L

/** Back on screen, the page is drawn again now and once more a moment later, when the web view
 * has its surface back. */
private const val REDRAW_AGAIN_MS = 150L

/** About the size the player shows a cover at. */
private const val COVER_PX = 512

/** The picture of `bytes`, halved as long as it stays as large as [COVER_PX]: a cover of
 * thousands of pixels would weigh megabytes, copied to the system with each change of it. */
private fun decodeCover(bytes: ByteArray): Bitmap? {
  val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
  BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
  var sample = 1
  while (maxOf(bounds.outWidth, bounds.outHeight) / (sample * 2) >= COVER_PX) sample *= 2
  val options = BitmapFactory.Options().apply { inSampleSize = sample }
  return BitmapFactory.decodeByteArray(bytes, 0, bytes.size, options)
}

class MainActivity : TauriActivity() {
  /** Lets the screen sleep, a moment after the last deck stopped. */
  private var lettingTheScreenSleep: Runnable? = null

  /** The page, drawn again when Prisme comes back on screen. */
  private var page: WebView? = null

  /**
   * The music shared between applications, and the notification of a deck playing: once the
   * DJ lets Prisme read the music, the native side looks at it again.
   */
  private val musicAndNotifications =
    registerForActivityResult(RequestMultiplePermissions()) { granted ->
      val music = listOf(Manifest.permission.READ_MEDIA_AUDIO, Manifest.permission.READ_EXTERNAL_STORAGE)
      if (music.any { granted[it] == true }) musicAllowed()
      answered()
    }

  /**
   * The inputs of a sound card: once the DJ says yes, the native side takes them, and the
   * service of a deck already playing says it listens too, before Prisme leaves the screen.
   */
  private val inputs =
    registerForActivityResult(RequestMultiplePermissions()) { granted ->
      if (granted[Manifest.permission.RECORD_AUDIO] == true) {
        inputsAllowed()
        if (PlaybackService.running) startForegroundService(Intent(this, PlaybackService::class.java))
      }
      answered()
    }

  /**
   * Back with nothing left to close (`back.ts`) sends Prisme behind. Android would end the
   * activity before Android 12, and from it on too when Prisme was opened other than from the
   * launcher (a sound card plugged in): the native side would end the process with it, a deck
   * playing falling silent.
   */
  private val sendBehind =
    object : OnBackPressedCallback(true) {
      override fun handleOnBackPressed() {
        moveTaskToBack(true)
      }
    }

  /**
   * The questions to the DJ, the one Android is asking first: it asks one at a time, and
   * refuses another meanwhile.
   */
  private val questions = ArrayDeque<Pair<ActivityResultLauncher<Array<String>>, Array<String>>>()

  override fun onCreate(savedInstanceState: Bundle?) {
    // light icons over the dark background of Prisme, whatever the theme of the phone
    val bars = SystemBarStyle.dark(Color.TRANSPARENT)
    enableEdgeToEdge(bars, bars)
    // the audio starts with the native side, which may open a USB card at once
    UsbAudio.init(this)
    // which asks for the inputs of a sound card through this activity
    shown = this
    // first, the last asked: the page goes back through what is open before
    onBackPressedDispatcher.addCallback(sendBehind)
    super.onCreate(savedInstanceState)
    // the screen is Prisme's up to its short edges, beside the camera too
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
      window.attributes.layoutInDisplayCutoutMode =
        WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES
    }
    keepClearOfTheBars()
    takeTheScreen(resources.configuration)
    askForTheMusicAndNotifications()
  }

  override fun onWebViewCreate(webView: WebView) {
    page = webView
  }

  override fun onDestroy() {
    page = null
    lettingTheScreenSleep?.let(window.decorView::removeCallbacks)
    if (shown === this) shown = null
    super.onDestroy()
  }

  override fun onConfigurationChanged(newConfig: Configuration) {
    super.onConfigurationChanged(newConfig)
    takeTheScreen(newConfig)
  }

  override fun onResume() {
    super.onResume()
    Update.back()
    redraw()
  }

  override fun onPause() {
    Update.left()
    super.onPause()
  }

  override fun onWindowFocusChanged(hasFocus: Boolean) {
    super.onWindowFocusChanged(hasFocus)
    if (!hasFocus) return
    // the bars come back with a dialog of the system: away again once it is gone
    takeTheScreen(resources.configuration)
    redraw()
  }

  /**
   * Back from another application or from the lock screen, the web view may show nothing until
   * it is touched: it waits for a reason to draw. It is given one.
   */
  private fun redraw() {
    val view = page ?: return
    val draw = Runnable {
      view.invalidate()
      view.requestLayout()
    }
    view.post(draw)
    view.postDelayed(draw, REDRAW_AGAIN_MS)
  }

  /**
   * Held sideways, the phone gives Prisme its whole screen: the bars of the system leave, and
   * come back for a moment when swiped in from an edge. Upright, they stay.
   */
  private fun takeTheScreen(configuration: Configuration) {
    val controller = WindowCompat.getInsetsController(window, window.decorView)
    controller.systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
    if (configuration.orientation == Configuration.ORIENTATION_LANDSCAPE) {
      controller.hide(WindowInsetsCompat.Type.systemBars())
    } else {
      controller.show(WindowInsetsCompat.Type.systemBars())
    }
  }

  /**
   * The page stays clear of the bars of the system while they are there. Upright, it stays
   * below the camera too; sideways, it goes under it to the very edge, as the camera only
   * takes a hole there.
   */
  private fun keepClearOfTheBars() {
    val content = findViewById<View>(android.R.id.content)
    ViewCompat.setOnApplyWindowInsetsListener(content) { view, insets ->
      val sideways = resources.configuration.orientation == Configuration.ORIENTATION_LANDSCAPE
      val bars = WindowInsetsCompat.Type.systemBars()
      val taken =
        insets.getInsets(if (sideways) bars else bars or WindowInsetsCompat.Type.displayCutout())
      view.setPadding(taken.left, taken.top, taken.right, taken.bottom)
      WindowInsetsCompat.CONSUMED
    }
  }

  /**
   * A set is played without touching the phone: its screen stays on while a deck plays, and a
   * moment after the last one stopped, the time to pick the next track.
   */
  private fun keepTheScreenOn(playing: Boolean) {
    val view = window.decorView
    val keptOn = (window.attributes.flags and WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON) != 0
    if (playing) {
      lettingTheScreenSleep?.let(view::removeCallbacks)
      lettingTheScreenSleep = null
      window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
    } else if (keptOn && lettingTheScreenSleep == null) {
      val sleep = Runnable {
        window.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        lettingTheScreenSleep = null
      }
      view.postDelayed(sleep, SCREEN_LINGER_MS)
      lettingTheScreenSleep = sleep
    }
  }

  /**
   * The music the phone shares between applications is read once the DJ allows it, and the
   * notification of a deck playing shown (without it, Android hides it). Both are asked for
   * at once, at the start rather than in the middle of a set; on Android 10, the recordings
   * written to the documents the phone shares too. (The inputs of a sound card are asked for
   * when one is first wanted: see `input_access.rs`.)
   */
  private fun askForTheMusicAndNotifications() {
    val wanted =
      when {
        Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU ->
          arrayOf(Manifest.permission.READ_MEDIA_AUDIO, Manifest.permission.POST_NOTIFICATIONS)
        Build.VERSION.SDK_INT >= Build.VERSION_CODES.R -> arrayOf(Manifest.permission.READ_EXTERNAL_STORAGE)
        else ->
          arrayOf(Manifest.permission.READ_EXTERNAL_STORAGE, Manifest.permission.WRITE_EXTERNAL_STORAGE)
      }
    val missing = wanted.filter { checkSelfPermission(it) != PackageManager.PERMISSION_GRANTED }
    if (missing.isNotEmpty()) ask(musicAndNotifications, missing.toTypedArray())
  }

  /** Asks the DJ through `question`, once any question asked before is answered. */
  private fun ask(question: ActivityResultLauncher<Array<String>>, permissions: Array<String>) {
    if (questions.any { it.first === question }) return
    questions.addLast(question to permissions)
    if (questions.size == 1) question.launch(permissions)
  }

  /** The question Android was asking is answered: the next one, if any. */
  private fun answered() {
    questions.removeFirstOrNull()
    questions.firstOrNull()?.let { (question, permissions) -> question.launch(permissions) }
  }

  /** The DJ let Prisme listen to the inputs (`input_access.rs`): the audio takes them. */
  private external fun inputsAllowed()

  /** The DJ let Prisme read the shared music (`music_access.rs`): the library looks again. */
  private external fun musicAllowed()

  companion object {
    /** The activity on screen, for the native side. */
    @Volatile
    var shown: MainActivity? = null
      private set

    /**
     * Asks the DJ to let Prisme listen to the inputs of the sound card, from any thread
     * (`input_access.rs`): on the main thread, where Android asks.
     */
    @JvmStatic
    fun askForTheInputs() {
      val activity = shown ?: return
      activity.runOnUiThread {
        activity.ask(activity.inputs, arrayOf(Manifest.permission.RECORD_AUDIO))
      }
    }

    /** What the native side says the [Player] shows (`lock_player.rs`), from any thread: the
     * screen stays on while a deck plays. */
    @JvmStatic
    fun showPlayer(context: Context, state: String) {
      val parsed = PlayerState.parse(state)
      Handler(Looper.getMainLooper()).post {
        Player.update(context, parsed)
        shown?.keepTheScreenOn(parsed.playing)
      }
    }

    /** The cover of `path` (`lock_player.rs`), from any thread; null when it has none. */
    @JvmStatic
    fun showCover(context: Context, path: String, jpeg: ByteArray?) {
      val bitmap = jpeg?.let(::decodeCover)
      Handler(Looper.getMainLooper()).post { Player.cover(context, path, bitmap) }
    }
  }
}
