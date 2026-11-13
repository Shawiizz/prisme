package com.shawiizz.prisme

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import android.view.WindowManager
import androidx.activity.enableEdgeToEdge

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    // a set is played without touching the phone: its screen stays on
    window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
    askForTheMusic()
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
