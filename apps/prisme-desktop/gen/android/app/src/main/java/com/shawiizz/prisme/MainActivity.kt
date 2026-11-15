package com.shawiizz.prisme

import android.Manifest
import android.content.pm.PackageManager
import android.content.res.Configuration
import android.graphics.BitmapFactory
import android.graphics.Color
import android.os.Build
import android.os.Bundle
import android.util.Base64
import android.view.View
import android.view.WindowManager
import android.webkit.JavascriptInterface
import android.webkit.WebView
import androidx.activity.SystemBarStyle
import androidx.activity.enableEdgeToEdge
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    // light icons over the dark background of Prisme, whatever the theme of the phone
    val bars = SystemBarStyle.dark(Color.TRANSPARENT)
    enableEdgeToEdge(bars, bars)
    // the audio starts with the native side, which may open a USB card at once
    UsbAudio.init(this)
    super.onCreate(savedInstanceState)
    // a set is played without touching the phone: its screen stays on
    window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
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
    webView.addJavascriptInterface(Decks(), "prismeDecks")
    Player.toggle = { deck -> webView.evaluateJavascript("window.prismePlayer?.toggle($deck)", null) }
  }

  override fun onConfigurationChanged(newConfig: Configuration) {
    super.onConfigurationChanged(newConfig)
    takeTheScreen(newConfig)
  }

  override fun onWindowFocusChanged(hasFocus: Boolean) {
    super.onWindowFocusChanged(hasFocus)
    // the bars come back with a dialog of the system: away again once it is gone
    if (hasFocus) takeTheScreen(resources.configuration)
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

  /** What the page says of the decks, for the [Player] (`lock-player.ts`). */
  private inner class Decks {
    @JavascriptInterface
    fun update(state: String) {
      val parsed = PlayerState.parse(state)
      runOnUiThread { Player.update(this@MainActivity, parsed) }
    }

    /** The cover of `path`, in base64; empty when it has none. */
    @JavascriptInterface
    fun cover(path: String, base64: String) {
      val bytes = if (base64.isEmpty()) null else Base64.decode(base64, Base64.DEFAULT)
      val bitmap = bytes?.let { BitmapFactory.decodeByteArray(it, 0, it.size) }
      runOnUiThread { Player.cover(this@MainActivity, path, bitmap) }
    }
  }

  /**
   * The music the phone shares between applications is read once the DJ allows it, and the
   * notification of a deck playing shown (without it, Android hides it). Both are asked for
   * at once, at the start rather than in the middle of a set. (The inputs of a sound card are
   * asked for when one is first wanted: see `input_access.rs`.)
   */
  private fun askForTheMusicAndNotifications() {
    val wanted =
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
        arrayOf(Manifest.permission.READ_MEDIA_AUDIO, Manifest.permission.POST_NOTIFICATIONS)
      } else {
        arrayOf(Manifest.permission.READ_EXTERNAL_STORAGE)
      }
    val missing = wanted.filter { checkSelfPermission(it) != PackageManager.PERMISSION_GRANTED }
    if (missing.isNotEmpty()) requestPermissions(missing.toTypedArray(), 0)
  }
}
