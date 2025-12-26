import { FC, useMemo } from "preact/compat";
import { Logs } from "../../../api/types";
import "./style.scss";
import classNames from "classnames";
import GroupLogsFieldRow from "./GroupLogsFieldRow";
import { useLocalStorageBoolean } from "../../../hooks/useLocalStorageBoolean";
import useDeviceDetect from "../../../hooks/useDeviceDetect";
import { ExtraFilter } from "../../../pages/OverviewPage/FiltersBar/types";

interface Props {
  log: Logs;
  hideGroupButton?: boolean;
  onApplyFilter?: (value: ExtraFilter) => void;
}

const GroupLogsFields: FC<Props> = ({ log, hideGroupButton, onApplyFilter }) => {
  const { isMobile } = useDeviceDetect();

  const sortedFields = useMemo(() => {
    return Object.entries(log)
      .sort(([aKey], [bKey]) => aKey.localeCompare(bKey));
  }, [log]);

  const [disabledHovers] = useLocalStorageBoolean("LOGS_DISABLED_HOVERS");

  return (
    <div
      className={classNames({
        "vm-group-logs-row-fields": true,
        "vm-group-logs-row-fields_mobile": isMobile,
        "vm-group-logs-row-fields_interactive": !disabledHovers
      })}
    >
      <table>
        <tbody>
          {sortedFields.map(([key, value]) => (
            <GroupLogsFieldRow
              key={key}
              field={key}
              value={value}
              hideGroupButton={hideGroupButton}
              onApplyFilter={onApplyFilter}
            />
          ))}
        </tbody>
      </table>
    </div>
  );
};

export default GroupLogsFields;
