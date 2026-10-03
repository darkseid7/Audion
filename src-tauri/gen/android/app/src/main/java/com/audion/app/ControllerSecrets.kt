package com.audion.app

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import java.io.File
import java.io.FileOutputStream
import java.io.IOException
import java.util.UUID
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/** Encrypted envelope storage; the native platform supplies its protected key. */
class ControllerSecrets(private val directory: File, private val key: (Boolean) -> SecretKey?) {
    constructor(context: Context) : this(
        File(context.noBackupFilesDir, "controller-credentials-v1"),
        { create -> platformKey(create) }
    )
    private fun file(hostId: String): File {
        val id = UUID.fromString(hostId)
        require(id.toString() == hostId && id != UUID(0, 0)) { "Invalid host identity." }
        return File(directory, "$hostId.bin")
    }

    fun saveCredentials(hostId: String, bytes: ByteArray) = synchronized(STORAGE_LOCK) {
        val destination = file(hostId)
        recover(destination)
        require(bytes.isNotEmpty() && bytes.size <= MAX_SECRET_BYTES) { "Invalid credential size." }
        val protectedKey = key(true) ?: throw IOException("Secure key unavailable.")
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        // The provider generates a fresh cryptographic IV on every save.
        cipher.init(Cipher.ENCRYPT_MODE, protectedKey)
        require(cipher.iv.size == IV_BYTES)
        cipher.updateAAD(aad(hostId))
        val encrypted = cipher.doFinal(bytes)
        if (!directory.isDirectory && !directory.mkdirs()) throw IOException("Secure storage unavailable.")
        val pending = File(destination.path + ".new")
        val backup = File(destination.path + ".bak")
        // Fully sync ciphertext before the commit. The old committed record is
        // recoverable if the process dies between either of the two renames.
        FileOutputStream(pending).use { output ->
            output.write(MAGIC)
            output.write(cipher.iv)
            output.write(encrypted)
            output.fd.sync()
        }
        if (destination.exists() && !destination.renameTo(backup)) {
            pending.delete()
            throw IOException("Secure storage backup failed.")
        }
        if (!pending.renameTo(destination)) {
            recover(destination)
            throw IOException("Secure storage commit failed.")
        }
        // Presence of the new primary is the commit point, never roll it back.
        remove(backup)
    }

    fun loadCredentials(hostId: String): ByteArray? = synchronized(STORAGE_LOCK) {
        val source = file(hostId)
        try { recover(source) } catch (_: IOException) { return@synchronized null }
        decrypt(source,hostId)
    }

    private fun decrypt(source: File, hostId: String): ByteArray? {
        if (!source.isFile) return null
        return try {
            // A missing key must never be regenerated while reading an old record.
            val protectedKey = key(false) ?: return null
            val envelope = ByteArray(MAX_ENVELOPE_BYTES + 1)
            val length = source.inputStream().use { input ->
                var count = 0
                while (count < envelope.size) {
                    val read = input.read(envelope, count, envelope.size - count)
                    if (read < 0) break
                    count += read
                }
                count
            }
            if (length !in (HEADER_BYTES + TAG_BYTES + 1)..MAX_ENVELOPE_BYTES) return null
            if (!envelope.copyOfRange(0, MAGIC.size).contentEquals(MAGIC)) return null
            val cipher = Cipher.getInstance("AES/GCM/NoPadding")
            cipher.init(Cipher.DECRYPT_MODE, protectedKey, GCMParameterSpec(128, envelope.copyOfRange(MAGIC.size, HEADER_BYTES)))
            cipher.updateAAD(aad(hostId))
            cipher.doFinal(envelope, HEADER_BYTES, length - HEADER_BYTES)
        } catch (_: Exception) {
            // Corruption, changed host/schema, invalidation and lost keys require pairing.
            null
        }
    }

    fun deleteCredentials(hostId: String) = synchronized(STORAGE_LOCK) {
        val source = file(hostId)
        // Delete recovery records first so a completed deletion cannot resurrect.
        remove(File(source.path + ".new"))
        remove(File(source.path + ".bak"))
        remove(source)
    }

    private fun recover(source: File) {
        val backup = File(source.path + ".bak")
        if (backup.exists()) {
            if (source.exists()) remove(backup)
            else if (!backup.renameTo(source)) throw IOException("Secure storage recovery failed.")
        }
        remove(File(source.path + ".new"))
    }

    private fun remove(file: File) {
        if (file.exists() && !file.delete()) throw IOException("Secure storage cleanup failed.")
    }

    private fun aad(hostId: String) = "audion-controller-credentials:v1:$hostId".toByteArray(Charsets.UTF_8)

    companion object {
        private val STORAGE_LOCK = Any()
        private const val MAX_SECRET_BYTES = 16_384
        private const val IV_BYTES = 12
        private const val TAG_BYTES = 16
        private const val HEADER_BYTES = 4 + IV_BYTES
        private const val MAX_ENVELOPE_BYTES = HEADER_BYTES + TAG_BYTES + MAX_SECRET_BYTES
        private val MAGIC = byteArrayOf(0x41, 0x55, 0x44, 0x01)
        private const val KEY_ALIAS = "audion.controller.credentials.v1"

        private fun platformKey(create: Boolean): SecretKey? {
            val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
            if (store.containsAlias(KEY_ALIAS)) return store.getKey(KEY_ALIAS, null) as? SecretKey
            if (!create) return null
            return KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore").apply {
                init(KeyGenParameterSpec.Builder(KEY_ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                    .setKeySize(256)
                    .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                    .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                    .setRandomizedEncryptionRequired(true)
                    .build())
            }.generateKey()
        }
    }
}
