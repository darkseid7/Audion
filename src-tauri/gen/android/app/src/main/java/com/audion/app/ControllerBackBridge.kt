package com.audion.app

/** Presentation-only Back arbitration. No playback, storage or domain startup. */
internal class ControllerBackBridge(private val fallback: () -> Unit) {
    private var generation = 0
    private var destroyed = false
    private var pending = false
    private var evaluate: ((String, (String?) -> Unit) -> Unit)? = null

    fun attach(evaluate: (String, (String?) -> Unit) -> Unit) {
        generation++
        pending = false
        this.evaluate = evaluate
    }

    fun back() {
        if (destroyed || pending) return
        val evaluator = evaluate
        if (evaluator == null) { fallback(); return }
        val origin = generation
        pending = true
        val reply: (String?) -> Unit = { result ->
            if (!destroyed && origin == generation && pending) {
                pending = false
                if (result != "true") fallback()
            }
        }
        try { evaluator(SCRIPT, reply) } catch (_: Exception) { reply(null) }
    }

    fun destroy() {
        destroyed = true
        generation++
        pending = false
        evaluate = null
    }

    companion object {
        const val SCRIPT = "(function(){try{return typeof window.__audionHandleBack==='function' && window.__audionHandleBack()===true;}catch(_){return false;}})()"
    }
}
