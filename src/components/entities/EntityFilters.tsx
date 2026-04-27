import type { EntityListQuery, EntityValidationStatus } from "../../types/domain";

const validationStatuses: EntityValidationStatus[] = ["valid", "warning", "invalid"];

interface EntityFiltersProps {
  query: EntityListQuery;
  availableStages: string[];
  availableStatuses: string[];
  onChange: (patch: Partial<EntityListQuery>) => void;
  onClear: () => void;
  disabled: boolean;
}

export function EntityFilters({
  query,
  availableStages,
  availableStatuses,
  onChange,
  onClear,
  disabled,
}: EntityFiltersProps) {
  return (
    <section className="panel">
      <div className="panel-heading">
        <h2>Filters</h2>
        <button type="button" className="button secondary" disabled={disabled} onClick={onClear}>
          Clear filters
        </button>
      </div>
      <div className="filter-grid">
        <div className="form-row">
          <label htmlFor="entity-search">Search</label>
          <input
            id="entity-search"
            value={query.search ?? ""}
            disabled={disabled}
            onChange={(event) => onChange({ search: event.target.value, page: 1 })}
            placeholder="Entity ID or file path"
          />
        </div>
        <div className="form-row">
          <label htmlFor="entity-stage-filter">Stage</label>
          <select
            id="entity-stage-filter"
            value={query.stage_id ?? ""}
            disabled={disabled}
            onChange={(event) => onChange({ stage_id: event.target.value || null, page: 1 })}
          >
            <option value="">All stages</option>
            {availableStages.map((stageId) => (
              <option key={stageId} value={stageId}>
                {stageId}
              </option>
            ))}
          </select>
        </div>
        <div className="form-row">
          <label htmlFor="entity-status-filter">Status</label>
          <select
            id="entity-status-filter"
            value={query.status ?? ""}
            disabled={disabled}
            onChange={(event) => onChange({ status: event.target.value || null, page: 1 })}
          >
            <option value="">All statuses</option>
            {availableStatuses.map((status) => (
              <option key={status} value={status}>
                {status}
              </option>
            ))}
          </select>
        </div>
        <div className="form-row">
          <label htmlFor="entity-validation-filter">Validation</label>
          <select
            id="entity-validation-filter"
            value={query.validation_status ?? ""}
            disabled={disabled}
            onChange={(event) =>
              onChange({
                validation_status: (event.target.value || null) as EntityValidationStatus | null,
                page: 1,
              })
            }
          >
            <option value="">All validation states</option>
            {validationStatuses.map((status) => (
              <option key={status} value={status}>
                {status}
              </option>
            ))}
          </select>
        </div>
      </div>
    </section>
  );
}

