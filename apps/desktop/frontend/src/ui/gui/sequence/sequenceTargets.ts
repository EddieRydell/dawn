import type { LayoutTargetDto } from "../../../types";

export function targetsEqual(left: LayoutTargetDto, right: LayoutTargetDto) {
  return left.kind === right.kind && left.name === right.name;
}
