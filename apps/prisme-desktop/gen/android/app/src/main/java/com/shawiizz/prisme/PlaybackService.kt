package com.shawiizz.prisme

import android.app.Service
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.IBinder

/**
 * Held while a deck plays, and a while after: the system keeps Prisme running, its sound
 * going on with the screen locked or another application in front, and shows its [Player]
 * on the lock screen and in the notifications.
 */
class PlaybackService : Service() {
  override fun onBind(intent: Intent?): IBinder? = null

  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    startForeground(NOTIFICATION, Player.notification(this), ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK)
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

  companion object {
    const val NOTIFICATION = 1
    /** The deck a button of the notification plays or pauses. */
    const val DECK = "deck"
    var running = false
      private set
  }
}
