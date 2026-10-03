import type { ApplicationPort } from "../types";
import { controllerSession, type ControllerNativeBridge } from "./session";
export function createControllerAdapter(native: ControllerNativeBridge): ApplicationPort {
    return controllerSession(native).port;
}
