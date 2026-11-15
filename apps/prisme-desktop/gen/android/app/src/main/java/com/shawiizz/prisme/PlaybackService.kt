package com.shawiizz.prisme

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.IBinder

/**
 * Held while a deck plays: the system keeps Prisme running, its sound going on with the screen
 * locked or another application in front, and says so in a notification that brings it back.
 */
class PlaybackService : Service() {
  override fun onBind(intent: Intent?): IBinder? = null

  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    val channel =
      NotificationChannel(CHANNEL, getString(R.string.playback_channel), NotificationManager.IMPORTANCE_LOW)
    getSystemService(NotificationManager::class.java).createNotificationChannel(channel)
    val back =
      PendingIntent.getActivity(
        this,
        0,
        Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
        PendingIntent.FLAG_IMMUTABLE,
      )
    val notification =
      Notification.Builder(this, CHANNEL)
        .setSmallIcon(R.drawable.ic_stat_playing)
        .setContentTitle(getString(R.string.app_name))
        .setContentText(getString(R.string.playback_text))
        .setContentIntent(back)
        .setOngoing(true)
        .build()
    startForeground(NOTIFICATION, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK)
    return START_NOT_STICKY
  }

  private companion object {
    const val CHANNEL = "playback"
    const val NOTIFICATION = 1
  }
}
