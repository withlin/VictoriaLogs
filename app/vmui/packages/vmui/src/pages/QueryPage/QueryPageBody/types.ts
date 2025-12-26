import { Logs } from "../../../api/types";
import { RefObject } from "preact/compat";
import { ExtraFilter } from "../../OverviewPage/FiltersBar/types";

export interface ViewProps {
  data: Logs[];
  settingsRef: RefObject<HTMLDivElement>;
  onApplyFilter?: (value: ExtraFilter) => void;
}
