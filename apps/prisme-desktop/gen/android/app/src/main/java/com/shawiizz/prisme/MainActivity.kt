package com.shawiizz.prisme

import android.Manifest
import android.content.pm.PackageManager
import android.graphics.Color
import android.os.Build
import android.os.Bundle
import android.view.View
import android.view.WindowManager
import androidx.activity.SystemBarStyle
import androidx.activity.enableEdgeToEdge
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    // light icons over the dark background of Prisme, whatever the theme of the phone
    val bars = SystemBarStyle.dark(Color.TRANSPARENT)
    enableEdgeToEdge(bars, bars)
    super.onCreate(savedInstanceState)
    // a set is played without touching the phone: its screen stays on
    window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
    keepClearOfTheBars()
    askForTheMusicAndTheInputs()
  }

  /** The page stays clear of the status bar, the notch and the gesture bar. */
  private fun keepClearOfTheBars() {
    val content = findViewById<View>(android.R.id.content)
    ViewCompat.setOnApplyWindowInsetsListener(content) { view, insets ->
      val taken =
        insets.getInsets(
          WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout()
        )
      view.setPadding(taken.left, taken.top, taken.right, taken.bottom)
      WindowInsetsCompat.CONSUMED
    }
  }

  /**
   * The music the phone shares between applications is read, and the inputs of a sound card
   * listened to (the timecode of the turntables), once the DJ allows it.
   */
  private fun askForTheMusicAndTheInputs() {
    val music =
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) Manifest.permission.READ_MEDIA_AUDIO
      else Manifest.permission.READ_EXTERNAL_STORAGE
    val missing =
      listOf(music, Manifest.permission.RECORD_AUDIO).filter {
        checkSelfPermission(it) != PackageManager.PERMISSION_GRANTED
      }
    if (missing.isNotEmpty()) {
      requestPermissions(missing.toTypedArray(), 0)
    }
  }
}
