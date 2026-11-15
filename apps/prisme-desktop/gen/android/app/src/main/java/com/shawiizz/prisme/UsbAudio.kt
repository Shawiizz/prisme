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
 * then one of them opened, its audio taken from Android's driver.
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
  private val opened = HashMap<String, UsbDeviceConnection>()
  /** Where Android's answer comes: not the main thread, which may be the one waiting for it. */
  private val answers by lazy { Handler(HandlerThread("prisme-usb-answers").apply { start() }.looper) }

  fun init(context: Context) {
    this.context = context.applicationContext
  }

  private val manager: UsbManager
    get() = context.getSystemService(UsbManager::class.java)

  /** The cards plugged in that speak the second USB audio class, by name. */
  @JvmStatic
  fun cards(): Array<String> = manager.deviceList.values.filter(::isAudioClass2).map(::nameOf).toTypedArray()

  /**
   * Opens `name`, asking the DJ the first time, and takes its audio from Android: its file
   * descriptor, -1 if it is not there or the DJ said no. Blocks until the DJ answered; never
   * to be called on the main thread.
   */
  @JvmStatic
  fun open(name: String): Int {
    val device = manager.deviceList.values.find { isAudioClass2(it) && nameOf(it) == name } ?: return -1
    if (!manager.hasPermission(device) && !ask(device)) return -1
    val connection = manager.openDevice(device) ?: return -1
    val audio = interfaces(device).filter { it.interfaceClass == UsbConstants.USB_CLASS_AUDIO }
    if (!audio.all { connection.claimInterface(it, true) }) {
      Log.w("Prisme", "the audio of $name could not be taken from Android")
      connection.close()
      return -1
    }
    synchronized(opened) { opened.put(name, connection)?.close() }
    return connection.fileDescriptor
  }

  /** The descriptors of `name`, opened: the device's, then its configuration's. */
  @JvmStatic
  fun descriptors(name: String): ByteArray? = synchronized(opened) { opened[name]?.rawDescriptors }

  /** Gives `name` back to Android. */
  @JvmStatic
  fun close(name: String) {
    val connection = synchronized(opened) { opened.remove(name) } ?: return
    val device = manager.deviceList.values.find { nameOf(it) == name }
    device?.let { interfaces(it).forEach(connection::releaseInterface) }
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

  private fun nameOf(device: UsbDevice): String = device.productName ?: device.deviceName
}
