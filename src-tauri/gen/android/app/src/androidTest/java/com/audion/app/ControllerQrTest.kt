package com.audion.app

import android.app.Activity
import android.content.Intent
import androidx.test.core.app.ActivityScenario
import androidx.test.core.app.ApplicationProvider
import androidx.test.espresso.Espresso.onView
import androidx.test.espresso.action.ViewActions.*
import androidx.test.espresso.assertion.ViewAssertions.matches
import androidx.test.espresso.matcher.ViewMatchers.*
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.uiautomator.By
import androidx.test.uiautomator.UiDevice
import androidx.test.uiautomator.Until
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class ControllerQrTest {
    private fun open(): ActivityScenario<ControllerCaptureActivity> =
        ActivityScenario.launchActivityForResult(Intent(ApplicationProvider.getApplicationContext(), ControllerCaptureActivity::class.java))

    @Test fun cancelReturnsNoInvitation() {
        open().use { scenario ->
            onView(withText("Cancel")).perform(click())
            assertEquals(Activity.RESULT_CANCELED, scenario.result.resultCode)
            assertNull(scenario.result.resultData)
        }
    }
    @Test fun rotationPreservesPasteFallback() {
        open().use { scenario ->
            onView(withHint("Paste invitation")).perform(replaceText("opaque invitation"), closeSoftKeyboard())
            scenario.recreate()
            onView(withHint("Paste invitation")).check(matches(withText("opaque invitation")))
            onView(withText("Use invitation")).perform(click())
            assertEquals("opaque invitation", scenario.result.resultData?.getStringExtra("invitation"))
        }
    }
    @Test fun oversizedPasteDoesNotReplaceOrRetainAnInvitation() {
        open().use { scenario ->
            onView(withHint("Paste invitation")).perform(replaceText("opaque invitation"), closeSoftKeyboard())
            onView(withHint("Paste invitation")).perform(replaceText("x".repeat(2049)), closeSoftKeyboard())
            onView(withHint("Paste invitation")).check(matches(withText("opaque invitation")))
            scenario.recreate()
            onView(withHint("Paste invitation")).check(matches(withText("opaque invitation")))
        }
    }
    @Test fun multibyteBudgetIsExactAndNeverSplitsAnEmoji() {
        val exact = "😀".repeat(512)
        open().use { scenario ->
            onView(withHint("Paste invitation")).perform(replaceText(exact), closeSoftKeyboard())
            onView(withHint("Paste invitation")).perform(replaceText(exact + "é"), closeSoftKeyboard())
            scenario.recreate()
            onView(withHint("Paste invitation")).check(matches(withText(exact)))
            onView(withText("Use invitation")).perform(click())
            assertEquals(exact, scenario.result.resultData?.getStringExtra("invitation"))
        }
    }
    @Test fun savedStateRejectsOversizedInputEvenIfTheEditFilterIsBypassed() {
        open().use { scenario ->
            scenario.onActivity { activity ->
                val field = ControllerCaptureActivity::class.java.getDeclaredField("input").apply { isAccessible = true }
                val input = field.get(activity) as android.widget.EditText
                assertFalse(input.isSaveEnabled)
                input.filters = emptyArray()
                input.setText("😀".repeat(513))
                val saved = android.os.Bundle()
                ControllerCaptureActivity::class.java.getDeclaredMethod("onSaveInstanceState", android.os.Bundle::class.java).apply { isAccessible = true }.invoke(activity, saved)
                assertEquals("", saved.getString("invitation"))
            }
            scenario.recreate()
            onView(withHint("Paste invitation")).check(matches(withText("")))
        }
    }
    @Test fun deniedCameraKeepsPasteFallbackAvailable() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val device = UiDevice.getInstance(instrumentation)
        device.executeShellCommand("pm revoke com.audion.app android.permission.CAMERA")
        open().use {
            onView(withText("Scan QR")).perform(click())
            val deny = device.wait(Until.findObject(By.res("com.android.permissioncontroller", "permission_deny_button")), 5000)
            assertNotNull("Camera permission prompt must be visible", deny)
            deny.click()
            onView(withHint("Paste invitation")).check(matches(isDisplayed()))
            onView(withHint("Paste invitation")).perform(replaceText("paste after denial"), closeSoftKeyboard())
            onView(withText("Use invitation")).perform(click())
            assertEquals(Activity.RESULT_OK, it.result.resultCode)
        }
    }
}
