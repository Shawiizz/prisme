package com.shawiizz.prisme

import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.hardware.usb.UsbConstants
import android.hardware.usb.UsbDevice
import android.hardware.usb.UsbDeviceConnection
import android.hardware.usb.UsbInterface
import android.hardware.usb.UsbManager
import android.os.Build
import android.os.Handler
import android.os.HandlerThread
import android.util.Log
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/**
 * The USB sound cards Prisme drives itself (`usb` in prisme-audio-io), as Android reaches some
 * of their channels only. The native side asks, from its audio thread: the cards plugged in,
 * then one of them opened, its audio taken from Android's driver. A card is known by the name
 * Android gives it, its own while it is plugged in (two of a product have the same name).
 */
object UsbAudio {
  private const val PERMISSION = "com.shawiizz.prisme.USB_PERMISSION"
  /** The audio class, its control and streaming interfaces, and its second version. */
  private const val AUDIO_CONTROL = 1
  private const val AUDIO_STREAMING = 2
  private const val AUDIO_CLASS_2 = 0x20
  /** How long the DJ has to answer Android's question, the first time. */
  private const val ANSWER_SECONDS = 120L

  private lateinit var context: Context
  /** The cards opened, by the name Android gives them. */
  private val opened = HashMap<String, UsbDeviceConnection>()
  /** Where Android's answer comes: not the main thread, which may be the one waiting for it. */
  private val answers by lazy { Handler(HandlerThread("prisme-usb-answers").apply { start() }.looper) }

  fun init(context: Context) {
    this.context = context.applicationContext
  }

  private val manager: UsbManager
    get() = context.getSystemService(UsbManager::class.java)

  /**
   * The cards plugged in that speak the second USB audio class: the name Android gives each,
   * then the name of its product, shown to the DJ.
   */
  @JvmStatic
  fun cards(): Array<String> =
    manager.deviceList.values
      .filter(::isAudioClass2)
      .flatMap { listOf(it.deviceName, it.productName ?: it.deviceName) }
      .toTypedArray()

  /**
   * Opens card `id`, asking the DJ the first time, and takes its audio from Android: its file
   * descriptor, -1 if it is not there or the DJ said no. Blocks until the DJ answered; never
   * to be called on the main thread.
   */
  @JvmStatic
  fun open(id: String): Int {
    val device = manager.deviceList[id]?.takeIf(::isAudioClass2) ?: return -1
    if (!manager.hasPermission(device) && !ask(device)) return -1
    val connection = manager.openDevice(device) ?: return -1
    val audio = interfaces(device).filter { it.interfaceClass == UsbConstants.USB_CLASS_AUDIO }
    if (!audio.all { connection.claimInterface(it, true) }) {
      Log.w("Prisme", "the audio of ${device.productName ?: id} could not be taken from Android")
      connection.close()
      return -1
    }
    synchronized(opened) { opened.put(id, connection)?.close() }
    return connection.fileDescriptor
  }

  /** The descriptors of card `id`, opened: the device's, then its configuration's. */
  @JvmStatic
  fun descriptors(id: String): ByteArray? = synchronized(opened) { opened[id]?.rawDescriptors }

  /** Gives card `id` back to Android. */
  @JvmStatic
  fun close(id: String) {
    val connection = synchronized(opened) { opened.remove(id) } ?: return
    manager.deviceList[id]?.let { interfaces(it).forEach(connection::releaseInterface) }
    connection.close()
  }

  private fun ask(device: UsbDevice): Boolean {
    val answered = CountDownLatch(1)
    var granted = false
    val receiver =
      object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
          granted = intent.getBooleanExtra(UsbManager.EXTRA_PERMISSION_GRANTED, false)
          answered.countDown()
        }
      }
    val filter = IntentFilter(PERMISSION)
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
      context.registerReceiver(receiver, filter, null, answers, Context.RECEIVER_NOT_EXPORTED)
    } else {
      context.registerReceiver(receiver, filter, null, answers)
    }
    try {
      val intent = Intent(PERMISSION).setPackage(context.packageName)
      manager.requestPermission(device, PendingIntent.getBroadcast(context, 0, intent, PendingIntent.FLAG_MUTABLE))
      answered.await(ANSWER_SECONDS, TimeUnit.SECONDS)
    } finally {
      context.unregisterReceiver(receiver)
    }
    return granted
  }

  private fun interfaces(device: UsbDevice): List<UsbInterface> =
    (0 until device.interfaceCount).map(device::getInterface).distinctBy { it.id }

  private fun isAudioClass2(device: UsbDevice): Boolean =
    interfaces(device).any {
      it.interfaceClass == UsbConstants.USB_CLASS_AUDIO &&
        it.interfaceSubclass == AUDIO_STREAMING &&
        it.interfaceProtocol == AUDIO_CLASS_2
    } && interfaces(device).any { it.interfaceSubclass == AUDIO_CONTROL }
}
