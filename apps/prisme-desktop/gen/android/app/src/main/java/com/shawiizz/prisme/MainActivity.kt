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
    askForTheMusic()
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

  /** The music the phone shares between applications is read once the DJ allows it. */
  private fun askForTheMusic() {
    val music =
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) Manifest.permission.READ_MEDIA_AUDIO
      else Manifest.permission.READ_EXTERNAL_STORAGE
    if (checkSelfPermission(music) != PackageManager.PERMISSION_GRANTED) {
      requestPermissions(arrayOf(music), 0)
    }
  }
}
