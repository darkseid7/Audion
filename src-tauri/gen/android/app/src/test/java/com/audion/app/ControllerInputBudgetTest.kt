package com.audion.app

import org.junit.Assert.*
import org.junit.Test

class ControllerInputBudgetTest {
    @Test fun preservesOpaqueWhitespaceAndMultibyteAtExactBudget() {
        val opaque = "  " + "😀".repeat(511) + "é"
        assertEquals(2048, opaque.toByteArray(Charsets.UTF_8).size)
        assertEquals(opaque, ControllerInputBudget.retain(opaque))
    }
    @Test fun rejectsWholeOversizedRestoreAndSaveRatherThanTruncatingInvitation() {
        assertEquals("", ControllerInputBudget.retain("x".repeat(2049)))
        assertEquals("", ControllerInputBudget.retain("😀".repeat(513)))
        assertEquals("", ControllerInputBudget.retain(null))
    }
    @Test fun rejectsOversizedPasteButAllowsReplacingSelectedText() {
        val full = "x".repeat(2048)
        assertFalse(ControllerInputBudget.permitsEdit(full, 2048, 2048, "é"))
        assertTrue(ControllerInputBudget.permitsEdit(full, 0, 2, "é"))
        assertFalse(ControllerInputBudget.permitsEdit("opaque", 0, 6, "x".repeat(3000)))
    }
    @Test fun rejectsSplitCodepointsWithoutMutatingValidEmoji() {
        assertEquals("", ControllerInputBudget.retain("\uD83D"))
        assertFalse(ControllerInputBudget.permitsEdit("😀", 1, 2, ""))
        assertTrue(ControllerInputBudget.permitsEdit("😀", 0, 2, "é"))
    }
    @Test fun preservesSmallPasteAndDeletionAcrossRetention() {
        val opaque=" opaque invitation "; assertEquals(opaque,ControllerInputBudget.retain(ControllerInputBudget.retain(opaque)))
        assertTrue(ControllerInputBudget.permitsEdit(opaque, 0, opaque.length, ""))
    }
}
