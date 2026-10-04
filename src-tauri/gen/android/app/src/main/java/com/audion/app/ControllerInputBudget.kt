package com.audion.app

/** Native capture retention only; never parses, normalizes or truncates invitations. */
internal object ControllerInputBudget {
    private const val MAX_BYTES = 2048
    private fun fits(value: CharSequence): Boolean {
        var bytes = 0
        var index = 0
        while (index < value.length) {
            val char = value[index++]
            bytes += when {
                char.code < 0x80 -> 1
                char.code < 0x800 -> 2
                Character.isHighSurrogate(char) -> {
                    if (index == value.length || !Character.isLowSurrogate(value[index])) return false
                    index++
                    4
                }
                Character.isLowSurrogate(char) -> return false
                else -> 3
            }
            if (bytes > MAX_BYTES) return false
        }
        return true
    }
    fun retain(value: String?): String = if (value != null && fits(value)) value else ""
    fun permitsEdit(destination: CharSequence, start: Int, end: Int, source: CharSequence): Boolean {
        // Check before constructing a candidate so a large paste cannot allocate a retained copy.
        if (!fits(source) || !fits(destination)) return false
        return fits(destination.subSequence(0, start).toString() + source + destination.subSequence(end, destination.length))
    }
}
