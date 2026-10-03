package com.audion.app

import java.io.File
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder

class ControllerSecretsEnvelopeTest {
    @get:Rule val temporary = TemporaryFolder()
    private val host = "cf8dd70c-8cc2-4640-bd40-5b06f68cc301"
    private val otherHost = "cf8dd70c-8cc2-4640-bd40-5b06f68cc302"
    private val secret = ByteArray(64) { (it + 1).toByte() }
    private fun key(): SecretKey = KeyGenerator.getInstance("AES").apply { init(256) }.generateKey()

    @Test fun interruptedReplacementRecoversLastCommittedCredentials() {
        val directory = temporary.newFolder()
        val key = key()
        val store = ControllerSecrets(directory) { key }
        store.saveCredentials(host, secret)
        val committed = File(directory, "$host.bin")
        assertTrue(committed.renameTo(File(directory, "$host.bin.bak")))
        File(directory, "$host.bin.new").writeBytes(byteArrayOf(1, 2, 3))
        val reopened = ControllerSecrets(directory) { key }
        assertArrayEquals(secret, reopened.loadCredentials(host))
        assertFalse(File(directory, "$host.bin.bak").exists())
        assertFalse(File(directory, "$host.bin.new").exists())
    }

    @Test fun completedReplacementNeverRollsBackToOldBackup() {
        val directory = temporary.newFolder()
        val key = key()
        val store = ControllerSecrets(directory) { key }
        store.saveCredentials(host, secret)
        val old = File(directory, "$host.bin").readBytes()
        val replacement = byteArrayOf(8, 9, 10)
        store.saveCredentials(host, replacement)
        File(directory, "$host.bin.bak").writeBytes(old)
        assertArrayEquals(replacement, ControllerSecrets(directory) { key }.loadCredentials(host))
        assertFalse(File(directory, "$host.bin.bak").exists())
    }

    @Test fun roundTripUsesFreshIvAndNeverStoresPlaintext() {
        val directory = temporary.newFolder()
        val key = key()
        val store = ControllerSecrets(directory) { key }
        store.saveCredentials(host, secret)
        assertArrayEquals(secret, store.loadCredentials(host))
        val first = directory.listFiles()!!.single().readBytes()
        assertFalse(first.toList().windowed(secret.size).any { it == secret.toList() })
        store.saveCredentials(host, secret)
        assertFalse(first.contentEquals(directory.listFiles()!!.single().readBytes()))
        assertArrayEquals(secret, store.loadCredentials(host))
        store.deleteCredentials(host)
        assertNull(store.loadCredentials(host))
    }
    @Test fun tamperedEnvelopeRequiresPairing() {
        val directory = temporary.newFolder()
        val key = key()
        val store = ControllerSecrets(directory) { key }
        for (offset in listOf(0, 4, 16, 32)) {
            store.saveCredentials(host, secret)
            assertArrayEquals(secret, store.loadCredentials(host))
            val file = directory.listFiles()!!.single()
            val bytes = file.readBytes()
            bytes[offset] = (bytes[offset].toInt() xor 1).toByte()
            file.writeBytes(bytes)
            assertNull("Changed envelope byte $offset must fail closed", store.loadCredentials(host))
        }
    }
    @Test fun crossHostDecryptRejected() {
        val directory = temporary.newFolder()
        val key = key()
        val store = ControllerSecrets(directory) { key }
        store.saveCredentials(host, secret)
        assertArrayEquals(secret, store.loadCredentials(host))
        val source = directory.listFiles()!!.single()
        store.saveCredentials(otherHost, secret)
        val target = directory.listFiles()!!.single { it != source }
        source.copyTo(target, overwrite = true)
        assertNull(store.loadCredentials(otherHost))
        assertArrayEquals(secret, store.loadCredentials(host))
    }
    @Test fun missingKeyRequiresPairingWithoutRegeneration() {
        val directory = temporary.newFolder()
        var current: SecretKey? = key()
        val store = ControllerSecrets(directory) { create ->
            if (current == null && create) fail("Loading must not regenerate a lost key")
            current
        }
        store.saveCredentials(host, secret)
        assertArrayEquals(secret, store.loadCredentials(host))
        current = null
        assertNull(store.loadCredentials(host))
    }
    @Test fun invalidHostAndOversizedCredentialsCannotWriteFiles() {
        val directory = temporary.newFolder()
        val key = key()
        val store = ControllerSecrets(directory) { key }
        for (hostId in listOf("../escape", "", "a/b")) {
            assertThrows(IllegalArgumentException::class.java) { store.saveCredentials(hostId, secret) }
        }
        assertThrows(IllegalArgumentException::class.java) { store.saveCredentials(host, ByteArray(16_385)) }
        assertTrue(directory.listFiles()!!.isEmpty())
    }
}
