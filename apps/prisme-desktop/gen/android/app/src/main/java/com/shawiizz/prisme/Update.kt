package com.shawiizz.prisme

import android.content.ActivityNotFoundException
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.provider.Settings
import android.util.Log
import androidx.core.content.FileProvider
import java.io.File
import java.util.concurrent.FutureTask

/**
 * A new version of Prisme, its APK downloaded by the native side into the cache
 * (`commands/apk.rs`), handed to Android's installer where the DJ confirms. Installed, it ends
 * Prisme, whose next start empties the cache; back in Prisme instead, the DJ did not install
 * it, and the file goes.
 */
object Update {
  /** [install]'s answers. */
  private const val OPENED = 0
  private const val NOT_ALLOWED = 1
  private const val NOT_PRISME = 2
  private const val NO_INSTALLER = 3

  private const val APK = "application/vnd.android.package-archive"

  /** The file the installer shows, and whether Prisme left the screen for it since. */
  private var handed: File? = null
  private var away = false

  /**
   * Hands the APK at `path` to Android's installer: [OPENED], [NOT_ALLOWED] while the DJ has
   * not let Prisme install applications, [NOT_PRISME] for a file that is no version of it,
   * [NO_INSTALLER] if nothing opened it. Waits for the main thread: never called on it.
   */
  @JvmStatic
  fun install(path: String): Int {
    val activity = MainActivity.shown ?: return NO_INSTALLER
    val packages = activity.packageManager
    if (!packages.canRequestPackageInstalls()) return NOT_ALLOWED
    if (archive(packages, path)?.packageName != activity.packageName) return NOT_PRISME
    val file = File(path)
    val uri = FileProvider.getUriForFile(activity, "${activity.packageName}.fileprovider", file)
    val intent = Intent(Intent.ACTION_VIEW).setDataAndType(uri, APK).addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
    val opened = FutureTask {
      try {
        activity.startActivity(intent)
        handed = file
        away = false
        OPENED
      } catch (e: ActivityNotFoundException) {
        Log.w("Prisme", "no installer opens the new version", e)
        NO_INSTALLER
      }
    }
    activity.runOnUiThread(opened)
    return opened.get()
  }

  /** Opens Android's setting letting Prisme install applications, for the DJ to allow it. */
  @JvmStatic
  fun allow() {
    val activity = MainActivity.shown ?: return
    val intent =
      Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:${activity.packageName}"))
    activity.runOnUiThread {
      try {
        activity.startActivity(intent)
      } catch (e: ActivityNotFoundException) {
        Log.w("Prisme", "no setting lets Prisme install applications", e)
      }
    }
  }

  /** Prisme leaves the screen, on the main thread. */
  fun left() {
    if (handed != null) away = true
  }

  /** Prisme is back on screen, on the main thread: back from the installer, nothing installed. */
  fun back() {
    if (!away) return
    handed?.delete()
    handed = null
    away = false
  }

  /** What the APK at `path` holds, if it is one. */
  private fun archive(packages: PackageManager, path: String) =
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
      packages.getPackageArchiveInfo(path, PackageManager.PackageInfoFlags.of(0))
    } else {
      @Suppress("DEPRECATION")
      packages.getPackageArchiveInfo(path, 0)
    }
}
