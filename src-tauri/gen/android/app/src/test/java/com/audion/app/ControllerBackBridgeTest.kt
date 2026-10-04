package com.audion.app
import org.junit.Assert.*
import org.junit.Test
class ControllerBackBridgeTest {
 @Test fun missingHelperAndMalformedRepliesFallBackButHandledDoesNot() {
  var fallback=0; val bridge=ControllerBackBridge { fallback++ }
  bridge.back(); assertEquals(1,fallback)
  for(result in listOf("false", "null", null, "true")) {
   bridge.attach { _,reply -> reply(result) }; bridge.back()
  }
  assertEquals(4,fallback)
 }
 @Test fun pendingBackCoalescesAndOldGenerationCannotMinimize() {
  var fallback=0; var calls=0; var reply: ((String?)->Unit)?=null
  val bridge=ControllerBackBridge { fallback++ }
  bridge.attach { _,callback -> calls++; reply=callback }
  bridge.back(); bridge.back(); assertEquals(1,calls)
  val old=reply!!; bridge.attach { _,callback -> reply=callback }; old("false"); assertEquals(0,fallback)
  bridge.back(); bridge.destroy(); reply!!("false"); bridge.back(); assertEquals(0,fallback)
 }
 @Test fun evaluatorFailureUsesRootFallback() {
  var fallback=0; val bridge=ControllerBackBridge { fallback++ }
  bridge.attach { _,_ -> throw IllegalStateException() }; bridge.back(); assertEquals(1,fallback)
 }
}
