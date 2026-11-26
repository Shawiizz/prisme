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
import androidx.core.content.IntentCompat

/**
 * The USB sound cards Prisme drives itself (`usb` in prisme-audio-io), as Android reaches some
 * of their channels only. The native side asks, from its audio thread: the cards plugged in,
 * whether Prisme may open one, the DJ's leave (the answer told back), then one opened, its
 * audio taken from Android's driver. A card is known by the name Android gives it, its own
 * while it is plugged in (two of a product have the same name).
 */
object UsbAudio {
  private const val PERMISSION = "com.shawiizz.prisme.USB_PERMISSION"
  /** The audio class, its control and streaming interfaces, and its second version. */
  private const val AUDIO_CONTROL = 1
  private const val AUDIO_STREAMING = 2
  private const val AUDIO_CLASS_2 = 0x20

  private lateinit var context: Context
  /** The cards opened, by the name Android gives them. */
  private val opened = HashMap<String, UsbDeviceConnection>()

  /**
   * Android's answers, told to the native side as they come, and the cards unplugged. Each
   * question of Android answers once, as it closes: no when the DJ left it or the card went.
   */
  private val news =
    object : BroadcastReceiver() {
      override fun onReceive(context: Context, intent: Intent) {
        val device = device(intent) ?: return
        when (intent.action) {
          PERMISSION ->
            answered(device.deviceName, intent.getBooleanExtra(UsbManager.EXTRA_PERMISSION_GRANTED, false))
          UsbManager.ACTION_USB_DEVICE_DETACHED -> unplugged(device.deviceName)
        }
      }
    }

  fun init(context: Context) {
    if (this::context.isInitialized) return
    this.context = context.applicationContext
    val filter = IntentFilter(PERMISSION).apply { addAction(UsbManager.ACTION_USB_DEVICE_DETACHED) }
    // off the main thread, busy with the page
    val thread = Handler(HandlerThread("prisme-usb-news").apply { start() }.looper)
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
      this.context.registerReceiver(news, filter, null, thread, Context.RECEIVER_NOT_EXPORTED)
    } else {
      this.context.registerReceiver(news, filter, null, thread)
    }
  }

  private val manager: UsbManager
    get() = context.getSystemService(UsbManager::class.java)

  /**
   * The cards plugged in that speak the second USB audio class: the name Android gives each,
   * the name of its product, shown to the DJ, and its model (USB vendor and product ids), by
   * which Prisme keeps it.
   */
  @JvmStatic
  fun cards(): Array<String> =
    manager.deviceList.values
      .filter(::isAudioClass2)
      .flatMap {
        listOf(it.deviceName, it.productName ?: it.deviceName, "%04x:%04x".format(it.vendorId, it.productId))
      }
      .toTypedArray()

  /** Whether Android lets Prisme open card `id`. */
  @JvmStatic
  fun allowed(id: String): Boolean = manager.deviceList[id]?.let(manager::hasPermission) ?: false

  /** Asks the DJ to let Prisme open card `id`: the answer is told back ([answered]). */
  @JvmStatic
  fun ask(id: String) {
    val device = manager.deviceList[id] ?: return
    val intent = Intent(PERMISSION).setPackage(context.packageName)
    manager.requestPermission(device, PendingIntent.getBroadcast(context, 0, intent, PendingIntent.FLAG_MUTABLE))
  }

  /**
   * Prisme opened for a card plugged in (`USB_DEVICE_ATTACHED`): Android lets Prisme open it
   * once the DJ said yes there, or at once if they said always. The native side opens it now,
   * its own question unanswered.
   */
  fun attached(intent: Intent) {
    if (intent.action != UsbManager.ACTION_USB_DEVICE_ATTACHED) return
    val device = device(intent) ?: return
    if (manager.hasPermission(device)) answered(device.deviceName, true)
  }

  /**
   * Opens card `id`, which Android lets Prisme open, and takes its audio from Android: its file
   * descriptor, -1 if it is not there or may not be opened.
   */
  @JvmStatic
  fun open(id: String): Int {
    val device = manager.deviceList[id]?.takeIf(::isAudioClass2) ?: return -1
    if (!manager.hasPermission(device)) return -1
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

  /** The DJ answered for card `id`, or opened Prisme for it (`usb_android.rs`). */
  @JvmStatic private external fun answered(id: String, allowed: Boolean)

  /** Card `id` was unplugged (`usb_android.rs`). */
  @JvmStatic private external fun unplugged(id: String)

  private fun device(intent: Intent): UsbDevice? =
    IntentCompat.getParcelableExtra(intent, UsbManager.EXTRA_DEVICE, UsbDevice::class.java)

  private fun interfaces(device: UsbDevice): List<UsbInterface> =
    (0 until device.interfaceCount).map(device::getInterface).distinctBy { it.id }

  private fun isAudioClass2(device: UsbDevice): Boolean =
    interfaces(device).any {
      it.interfaceClass == UsbConstants.USB_CLASS_AUDIO &&
        it.interfaceSubclass == AUDIO_STREAMING &&
        it.interfaceProtocol == AUDIO_CLASS_2
    } && interfaces(device).any { it.interfaceSubclass == AUDIO_CONTROL }
}
