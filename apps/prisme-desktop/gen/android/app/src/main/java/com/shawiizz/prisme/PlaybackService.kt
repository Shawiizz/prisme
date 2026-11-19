package com.shawiizz.prisme

import android.Manifest
import android.app.Service
import android.content.Intent
import android.content.pm.PackageManager
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder

/**
 * Held while a deck plays, and a while after: the system keeps Prisme running, its sound
 * going on with the screen locked or another application in front, and shows its [Player]
 * on the lock screen and in the notifications.
 */
class PlaybackService : Service() {
  override fun onBind(intent: Intent?): IBinder? = null

  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    val notification = Player.notification(this)
    try {
      startForeground(NOTIFICATION, notification, types())
    } catch (e: SecurityException) {
      // started out of sight, where Android gives no input: the sound alone
      startForeground(NOTIFICATION, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK)
    }
    running = true
    // a button of the notification
    intent?.getIntExtra(DECK, -1)?.takeIf { it >= 0 }?.let(Player::toggleDeck)
    return START_NOT_STICKY
  }

  override fun onDestroy() {
    running = false
    Player.release()
    super.onDestroy()
  }

  /**
   * The sound, and the inputs of the sound card once the DJ let Prisme listen to them: from
   * Android 11, an application out of sight hears only silence from its inputs unless a
   * service of its own says it listens.
   */
  private fun types(): Int {
    val sound = ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK
    val listens =
      Build.VERSION.SDK_INT >= Build.VERSION_CODES.R &&
        checkSelfPermission(Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED
    return if (listens) sound or ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE else sound
  }

  companion object {
    const val NOTIFICATION = 1
    /** The deck a button of the notification plays or pauses. */
    const val DECK = "deck"
    var running = false
      private set
  }
}
