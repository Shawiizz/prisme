package com.shawiizz.prisme

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.drawable.Icon
import android.media.MediaMetadata
import android.media.session.MediaSession
import android.media.session.PlaybackState
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.util.Log
import org.json.JSONObject

/** A deck as the page describes it (`lock-player.ts`). */
data class PlayerDeck(val deck: Int, val title: String, val artist: String?, val playing: Boolean) {
  val letter: String
    get() = "ABCD"[deck].toString()
}

/** What the page says the player shows: the decks loaded, the one heard and where it is. */
data class PlayerState(
  val decks: List<PlayerDeck>,
  val shown: Int?,
  val path: String?,
  val positionS: Double,
  val durationS: Double,
  val speed: Double,
  val controls: Boolean,
) {
  val playing: Boolean
    get() = decks.any { it.playing }

  val shownDeck: PlayerDeck?
    get() = decks.find { it.deck == shown }

  companion object {
    fun parse(json: String): PlayerState {
      val o = JSONObject(json)
      val decks = o.getJSONArray("decks")
      return PlayerState(
        decks =
          (0 until decks.length()).map {
            val d = decks.getJSONObject(it)
            PlayerDeck(
              d.getInt("deck"),
              d.getString("title"),
              d.optString("artist").takeUnless { d.isNull("artist") || it.isEmpty() },
              d.getBoolean("playing"),
            )
          },
        shown = if (o.isNull("shown")) null else o.getInt("shown"),
        path = if (o.isNull("path")) null else o.getString("path"),
        positionS = o.getDouble("positionS"),
        durationS = o.getDouble("durationS"),
        speed = o.getDouble("speed"),
        controls = o.getBoolean("controls"),
      )
    }
  }
}

/**
 * The player of the lock screen and of the notifications: the track heard, its cover and
 * where it is, and a button to play or pause each deck. [PlaybackService] holds it while a
 * deck plays, and a few minutes after the last one stopped, so that it can start again from
 * there.
 */
object Player {
  /** Plays or pauses a deck: the page does it (`MainActivity`). */
  var toggle: ((Int) -> Unit)? = null

  private var state: PlayerState? = null
  /** The cover of a file, null when it has none. */
  private var cover: Pair<String, Bitmap?>? = null
  private var session: MediaSession? = null
  private var art: Bitmap? = null
  private val main = Handler(Looper.getMainLooper())
  private var stopping: Runnable? = null

  private const val CHANNEL = "playback"
  private const val TOGGLE = "toggle"
  /** How long the player stays once every deck stopped. */
  private const val LINGER_MS = 5 * 60 * 1000L

  fun update(context: Context, state: PlayerState) {
    this.state = state
    show(context)
    follow(context, state)
  }

  fun cover(context: Context, path: String, bitmap: Bitmap?) {
    cover = path to bitmap
    if (state?.path == path) show(context)
  }

  /** Plays or pauses `deck`, if the DJ lets the player do it. */
  fun toggleDeck(deck: Int) {
    if (state?.controls == true) main.post { toggle?.invoke(deck) }
  }

  fun notification(context: Context): Notification {
    val manager = context.getSystemService(NotificationManager::class.java)
    manager.createNotificationChannel(
      NotificationChannel(CHANNEL, context.getString(R.string.playback_channel), NotificationManager.IMPORTANCE_LOW)
    )
    val state = state
    val shown = state?.shownDeck
    val back =
      PendingIntent.getActivity(
        context,
        0,
        Intent(context, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
        PendingIntent.FLAG_IMMUTABLE,
      )
    val buttons = buttons(context, state)
    val style =
      Notification.MediaStyle()
        .setMediaSession(session(context).sessionToken)
        .setShowActionsInCompactView(*buttons.indices.take(3).toIntArray())
    return Notification.Builder(context, CHANNEL)
      .setSmallIcon(R.drawable.ic_stat_playing)
      .setContentTitle(shown?.title ?: context.getString(R.string.app_name))
      .setContentText(shown?.let { subtitle(context, it) } ?: context.getString(R.string.playback_text))
      .setLargeIcon(artwork(context))
      .setContentIntent(back)
      .setOngoing(true)
      .setVisibility(Notification.VISIBILITY_PUBLIC)
      .setCategory(Notification.CATEGORY_TRANSPORT)
      .setStyle(style)
      .apply { buttons.forEach { addAction(it) } }
      .build()
  }

  /** Starts the service with the first deck to play; stops it a while after the last one
   * stopped, at once when no track is left. */
  private fun follow(context: Context, state: PlayerState) {
    stopping?.let(main::removeCallbacks)
    stopping = null
    val service = Intent(context, PlaybackService::class.java)
    when {
      state.playing ->
        if (!PlaybackService.running) {
          try {
            context.startForegroundService(service)
          } catch (e: IllegalStateException) {
            // a deck started from a controller while Prisme was out of sight
            Log.w("Prisme", "the sound may stop with the screen locked: ${e.message}")
          }
        }
      state.decks.isEmpty() -> context.stopService(service)
      else -> stopping = Runnable { context.stopService(service) }.also { main.postDelayed(it, LINGER_MS) }
    }
  }

  private fun show(context: Context) {
    val state = state ?: return
    val session = session(context)
    session.setMetadata(metadata(context, state))
    session.setPlaybackState(playback(context, state))
    if (PlaybackService.running) {
      context.getSystemService(NotificationManager::class.java).notify(PlaybackService.NOTIFICATION, notification(context))
    }
  }

  private fun session(context: Context): MediaSession =
    session
      ?: MediaSession(context.applicationContext, "Prisme").also {
        it.setCallback(
          object : MediaSession.Callback() {
            override fun onCustomAction(action: String, extras: Bundle?) {
              action.removePrefix(TOGGLE).toIntOrNull()?.let(::toggleDeck)
            }
          }
        )
        it.isActive = true
        session = it
      }

  private fun metadata(context: Context, state: PlayerState): MediaMetadata {
    val shown = state.shownDeck
    return MediaMetadata.Builder()
      .putString(MediaMetadata.METADATA_KEY_TITLE, shown?.title ?: context.getString(R.string.app_name))
      .putString(MediaMetadata.METADATA_KEY_ARTIST, shown?.let { subtitle(context, it) } ?: "")
      .putLong(MediaMetadata.METADATA_KEY_DURATION, (state.durationS * 1000).toLong())
      .putBitmap(MediaMetadata.METADATA_KEY_ART, artwork(context))
      .build()
  }

  private fun playback(context: Context, state: PlayerState): PlaybackState {
    val builder =
      PlaybackState.Builder()
        .setState(
          if (state.playing) PlaybackState.STATE_PLAYING else PlaybackState.STATE_PAUSED,
          (state.positionS * 1000).toLong(),
          state.speed.toFloat(),
        )
    if (state.controls) {
      state.decks.forEach { d ->
        builder.addCustomAction(
          PlaybackState.CustomAction.Builder(TOGGLE + d.deck, label(context, d), icon(d)).build()
        )
      }
    }
    return builder.build()
  }

  /** A button for each deck loaded, in the notification as on the lock screen. */
  private fun buttons(context: Context, state: PlayerState?): List<Notification.Action> {
    if (state == null || !state.controls) return emptyList()
    return state.decks.map { d ->
      val intent =
        Intent(context, PlaybackService::class.java).putExtra(PlaybackService.DECK, d.deck)
      val pending =
        PendingIntent.getService(context, d.deck + 1, intent, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
      Notification.Action.Builder(Icon.createWithResource(context, icon(d)), label(context, d), pending).build()
    }
  }

  private fun subtitle(context: Context, d: PlayerDeck): String {
    val deck = context.getString(R.string.player_deck, d.letter)
    return d.artist?.let { "$it · $deck" } ?: deck
  }

  private fun label(context: Context, d: PlayerDeck): String =
    context.getString(if (d.playing) R.string.player_pause else R.string.player_play, d.letter)

  private fun icon(d: PlayerDeck): Int =
    when (d.deck to d.playing) {
      0 to true -> R.drawable.ic_deck_a_pause
      0 to false -> R.drawable.ic_deck_a_play
      1 to true -> R.drawable.ic_deck_b_pause
      1 to false -> R.drawable.ic_deck_b_play
      2 to true -> R.drawable.ic_deck_c_pause
      2 to false -> R.drawable.ic_deck_c_play
      3 to true -> R.drawable.ic_deck_d_pause
      else -> R.drawable.ic_deck_d_play
    }

  /** The cover of the track shown, else the logo of Prisme. */
  private fun artwork(context: Context): Bitmap? {
    val own = cover?.takeIf { it.first == state?.path }?.second
    return own ?: art ?: BitmapFactory.decodeResource(context.resources, R.drawable.player_art).also { art = it }
  }
}
