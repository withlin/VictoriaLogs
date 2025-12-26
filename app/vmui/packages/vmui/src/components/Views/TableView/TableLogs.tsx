import { FC, useMemo, useEffect, useRef, useState, MouseEvent } from "preact/compat";
import Table from "../../Table/Table";
import { Logs } from "../../../api/types";
import Pagination from "../../Main/Pagination/Pagination";
import { ExtraFilter, ExtraFilterOperator } from "../../../pages/OverviewPage/FiltersBar/types";
import Tooltip from "../../Main/Tooltip/Tooltip";
import Button from "../../Main/Button/Button";
import { ZoomInIcon } from "../../Main/Icons";

interface TableLogsProps {
  logs: Logs[];
  displayColumns: string[];
  tableCompact: boolean;
  columns: string[];
  rowsPerPage: number;
  onApplyFilter?: (value: ExtraFilter) => void;
}

const getColumnClass = (key: string) => {
  switch (key) {
    case "_time":
      return "vm-table-cell_logs-time";
    default:
      return "vm-table-cell_logs";
  }
};

const compactColumns = [{
  key: "_vmui_data",
  title: "Data",
  className: "vm-table-cell_logs vm-table-cell_pre"
}];

const TableLogs: FC<TableLogsProps> = ({ logs, displayColumns, tableCompact, columns, rowsPerPage, onApplyFilter }) => {
  const containerRef = useRef<HTMLDivElement>(null);
  const [page, setPage] = useState(1);

  const rows = useMemo(() => {
    return logs.map((log) => {
      const _vmui_data = JSON.stringify(log, null, 2);
      return { ...log, _vmui_data };
    }) as Logs[];
  }, [logs]);

  const createCellRenderer = (field: string) => (log: Logs) => {
    const rawValue = log[field];
    const value = rawValue ?? "-";

    const handleFilter = (operator: ExtraFilterOperator) => (e: MouseEvent) => {
      e.stopPropagation();
      if (!onApplyFilter || rawValue === undefined) return;
      onApplyFilter({
        field,
        value: String(rawValue),
        operator
      });
    };

    return (
      <div className="vm-table-logs-cell">
        <span className="vm-table-logs-cell__value">
          {value}
        </span>
        {onApplyFilter && value !== "-" && (
          <div className="vm-table-logs-cell__actions">
            <Tooltip title="Add to query">
              <Button
                variant="text"
                color="gray"
                size="small"
                startIcon={<ZoomInIcon/>}
                onClick={handleFilter(ExtraFilterOperator.Equals)}
                ariaLabel="add to query"
              />
            </Tooltip>
          </div>
        )}
      </div>
    );
  };

  const tableColumns = useMemo(() => {
    return columns.map((key) => ({
      key: key as keyof Logs,
      title: key,
      className: getColumnClass(key),
      render: createCellRenderer(key),
    }));
  }, [columns, onApplyFilter]);


  const filteredColumns = useMemo(() => {
    if (tableCompact) return compactColumns;
    if (!displayColumns?.length) return [];
    return tableColumns.filter(c => displayColumns.includes(c.key as string));
  }, [tableColumns, displayColumns, tableCompact]);

  const paginationOffset = useMemo(() => {
    const startIndex = (page - 1) * rowsPerPage;
    const endIndex = startIndex + rowsPerPage;
    return { startIndex, endIndex };
  }, [page, rowsPerPage]);

  const handlePageChange = (newPage: number) => {
    setPage(newPage);
    if (containerRef.current) {
      const y = containerRef.current.getBoundingClientRect().top + window.scrollY - 50;
      window.scrollTo({ top: y });
    }
  };

  useEffect(() => {
    setPage(1);
  }, [logs, rowsPerPage]);

  return (
    <>
      <div ref={containerRef}>
        <Table
          rows={rows}
          columns={filteredColumns}
          defaultOrderBy={"_time"}
          defaultOrderDir={"desc"}
          copyToClipboard={"_vmui_data"}
          paginationOffset={paginationOffset}
        />
      </div>
      <Pagination
        currentPage={page}
        totalItems={rows.length}
        itemsPerPage={rowsPerPage}
        onPageChange={handlePageChange}
      />
    </>
  );
};

export default TableLogs;
