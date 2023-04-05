use crate::entities::filter_entities::*;
use crate::entities::{FieldType, InsertedRowPB, RowPB};
use crate::services::cell::{AnyTypeCache, AtomicCellDataCache, AtomicCellFilterCache};
use crate::services::database_view::{DatabaseViewChanged, DatabaseViewChangedNotifier};
use crate::services::field::*;
use crate::services::filter::{
  FilterChangeset, FilterResult, FilterResultNotification, FilterType,
};
use collab_database::fields::Field;
use collab_database::rows::{Cell, Row};
use collab_database::views::Filter;
use dashmap::DashMap;
use flowy_error::FlowyResult;
use flowy_task::{QualityOfService, Task, TaskContent, TaskDispatcher};
use lib_infra::future::Fut;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::RwLock;

type RowId = String;
pub trait FilterDelegate: Send + Sync + 'static {
  fn get_filter_rev(&self, filter_type: FilterType) -> Fut<Option<Arc<Filter>>>;
  fn get_field_rev(&self, field_id: &str) -> Fut<Option<Arc<Field>>>;
  fn get_field_revs(&self, field_ids: Option<Vec<String>>) -> Fut<Vec<Arc<Field>>>;
  fn get_rows(&self) -> Fut<Vec<Row>>;
  fn get_row(&self, rows_id: &str) -> Fut<Option<(usize, Arc<Row>)>>;
}

pub trait FromFilterString {
  fn from_filter(filter: &Filter) -> Self
  where
    Self: Sized;
}

pub struct FilterController {
  view_id: String,
  handler_id: String,
  delegate: Box<dyn FilterDelegate>,
  result_by_row_id: DashMap<RowId, FilterResult>,
  cell_data_cache: AtomicCellDataCache,
  cell_filter_cache: AtomicCellFilterCache,
  task_scheduler: Arc<RwLock<TaskDispatcher>>,
  notifier: DatabaseViewChangedNotifier,
}

impl Drop for FilterController {
  fn drop(&mut self) {
    tracing::trace!("Drop {}", std::any::type_name::<Self>());
  }
}

impl FilterController {
  pub async fn new<T>(
    view_id: &str,
    handler_id: &str,
    delegate: T,
    task_scheduler: Arc<RwLock<TaskDispatcher>>,
    filters: Vec<Arc<Filter>>,
    cell_data_cache: AtomicCellDataCache,
    notifier: DatabaseViewChangedNotifier,
  ) -> Self
  where
    T: FilterDelegate + 'static,
  {
    let this = Self {
      view_id: view_id.to_string(),
      handler_id: handler_id.to_string(),
      delegate: Box::new(delegate),
      result_by_row_id: DashMap::default(),
      cell_data_cache,
      cell_filter_cache: AnyTypeCache::<FilterType>::new(),
      task_scheduler,
      notifier,
    };
    this.refresh_filters(filters).await;
    this
  }

  pub async fn close(&self) {
    if let Ok(mut task_scheduler) = self.task_scheduler.try_write() {
      task_scheduler.unregister_handler(&self.handler_id).await;
    } else {
      tracing::error!("Try to get the lock of task_scheduler failed");
    }
  }

  #[tracing::instrument(name = "schedule_filter_task", level = "trace", skip(self))]
  async fn gen_task(&self, task_type: FilterEvent, qos: QualityOfService) {
    let task_id = self.task_scheduler.read().await.next_task_id();
    let task = Task::new(
      &self.handler_id,
      task_id,
      TaskContent::Text(task_type.to_string()),
      qos,
    );
    self.task_scheduler.write().await.add_task(task);
  }

  pub async fn filter_row_revs(&self, row_revs: &mut Vec<Arc<Row>>) {
    if self.cell_filter_cache.read().is_empty() {
      return;
    }
    let field_by_field_id = self.get_field_map().await;
    row_revs.iter().for_each(|row| {
      let _ = filter_row(
        row,
        &self.result_by_row_id,
        &field_by_field_id,
        &self.cell_data_cache,
        &self.cell_filter_cache,
      );
    });

    row_revs.retain(|row_rev| {
      self
        .result_by_row_id
        .get(&row_rev.id)
        .map(|result| result.is_visible())
        .unwrap_or(false)
    });
  }

  async fn get_field_map(&self) -> HashMap<String, Arc<Field>> {
    self
      .delegate
      .get_field_revs(None)
      .await
      .into_iter()
      .map(|field| (field.id.clone(), field))
      .collect::<HashMap<String, Arc<Field>>>()
  }

  #[tracing::instrument(
    name = "process_filter_task",
    level = "trace",
    skip_all,
    fields(filter_result),
    err
  )]
  pub async fn process(&self, predicate: &str) -> FlowyResult<()> {
    let event_type = FilterEvent::from_str(predicate).unwrap();
    match event_type {
      FilterEvent::FilterDidChanged => self.filter_all_rows().await?,
      FilterEvent::RowDidChanged(row_id) => self.filter_row(row_id).await?,
    }
    Ok(())
  }

  async fn filter_row(&self, row_id: String) -> FlowyResult<()> {
    if let Some((_, row)) = self.delegate.get_row(&row_id).await {
      let field_by_field_id = self.get_field_map().await;
      let mut notification = FilterResultNotification::new(self.view_id.clone());
      if let Some((row_id, is_visible)) = filter_row(
        &row,
        &self.result_by_row_id,
        &field_by_field_id,
        &self.cell_data_cache,
        &self.cell_filter_cache,
      ) {
        if is_visible {
          if let Some((index, row)) = self.delegate.get_row(&row_id).await {
            let row_pb = RowPB::from(row.as_ref());
            notification
              .visible_rows
              .push(InsertedRowPB::with_index(row_pb, index as i32))
          }
        } else {
          notification.invisible_rows.push(row_id);
        }
      }

      let _ = self
        .notifier
        .send(DatabaseViewChanged::FilterNotification(notification));
    }
    Ok(())
  }

  async fn filter_all_rows(&self) -> FlowyResult<()> {
    let field_by_field_id = self.get_field_map().await;
    let mut visible_rows = vec![];
    let mut invisible_rows = vec![];

    for (index, row) in self.delegate.get_rows().await.into_iter().enumerate() {
      if let Some((row_id, is_visible)) = filter_row(
        &row,
        &self.result_by_row_id,
        &field_by_field_id,
        &self.cell_data_cache,
        &self.cell_filter_cache,
      ) {
        if is_visible {
          let row_pb = RowPB::from(&row);
          visible_rows.push(InsertedRowPB::with_index(row_pb, index as i32))
        } else {
          invisible_rows.push(row_id);
        }
      }
    }

    let notification = FilterResultNotification {
      view_id: self.view_id.clone(),
      invisible_rows,
      visible_rows,
    };
    tracing::Span::current().record("filter_result", format!("{:?}", &notification).as_str());
    let _ = self
      .notifier
      .send(DatabaseViewChanged::FilterNotification(notification));
    Ok(())
  }

  pub async fn did_receive_row_changed(&self, row_id: &str) {
    self
      .gen_task(
        FilterEvent::RowDidChanged(row_id.to_string()),
        QualityOfService::UserInteractive,
      )
      .await
  }

  #[tracing::instrument(level = "trace", skip(self))]
  pub async fn did_receive_changes(
    &self,
    changeset: FilterChangeset,
  ) -> Option<FilterChangesetNotificationPB> {
    let mut notification: Option<FilterChangesetNotificationPB> = None;
    if let Some(filter_type) = &changeset.insert_filter {
      if let Some(filter) = self.filter_from_filter_type(filter_type).await {
        notification = Some(FilterChangesetNotificationPB::from_insert(
          &self.view_id,
          vec![filter],
        ));
      }
      if let Some(filter) = self.delegate.get_filter_rev(filter_type.clone()).await {
        self.refresh_filters(vec![filter]).await;
      }
    }

    if let Some(updated_filter_type) = changeset.update_filter {
      if let Some(old_filter_type) = updated_filter_type.old {
        let new_filter = self.filter_from_filter_type(&updated_filter_type.new).await;
        let old_filter = self.filter_from_filter_type(&old_filter_type).await;

        // Get the filter id
        let mut filter_id = old_filter.map(|filter| filter.id);
        if filter_id.is_none() {
          filter_id = new_filter.as_ref().map(|filter| filter.id.clone());
        }

        // Update the corresponding filter in the cache
        if let Some(filter) = self
          .delegate
          .get_filter_rev(updated_filter_type.new.clone())
          .await
        {
          self.refresh_filters(vec![filter]).await;
        }

        if let Some(filter_id) = filter_id {
          notification = Some(FilterChangesetNotificationPB::from_update(
            &self.view_id,
            vec![UpdatedFilter {
              filter_id,
              filter: new_filter,
            }],
          ));
        }
      }
    }

    if let Some(filter_type) = &changeset.delete_filter {
      if let Some(filter) = self.filter_from_filter_type(filter_type).await {
        notification = Some(FilterChangesetNotificationPB::from_delete(
          &self.view_id,
          vec![filter],
        ));
      }
      self.cell_filter_cache.write().remove(filter_type);
    }

    self
      .gen_task(FilterEvent::FilterDidChanged, QualityOfService::Background)
      .await;
    tracing::trace!("{:?}", notification);
    notification
  }

  async fn filter_from_filter_type(&self, filter_type: &FilterType) -> Option<FilterPB> {
    self
      .delegate
      .get_filter_rev(filter_type.clone())
      .await
      .map(|filter| FilterPB::from(filter.as_ref()))
  }

  #[tracing::instrument(level = "trace", skip_all)]
  async fn refresh_filters(&self, filters: Vec<Arc<Filter>>) {
    for filter in filters {
      if let Some(field) = self.delegate.get_field_rev(&filter.field_id).await {
        let filter_type = FilterType::from(field.as_ref());
        tracing::trace!("Create filter with type: {:?}", filter_type);
        match &filter_type.field_type {
          FieldType::RichText => {
            self
              .cell_filter_cache
              .write()
              .insert(&filter_type, TextFilterPB::from_filter(filter.as_ref()));
          },
          FieldType::Number => {
            self
              .cell_filter_cache
              .write()
              .insert(&filter_type, NumberFilterPB::from_filter(filter.as_ref()));
          },
          FieldType::DateTime => {
            self
              .cell_filter_cache
              .write()
              .insert(&filter_type, DateFilterPB::from_filter(filter.as_ref()));
          },
          FieldType::SingleSelect | FieldType::MultiSelect => {
            self.cell_filter_cache.write().insert(
              &filter_type,
              SelectOptionFilterPB::from_filter(filter.as_ref()),
            );
          },
          FieldType::Checkbox => {
            self
              .cell_filter_cache
              .write()
              .insert(&filter_type, CheckboxFilterPB::from_filter(filter.as_ref()));
          },
          FieldType::URL => {
            self
              .cell_filter_cache
              .write()
              .insert(&filter_type, TextFilterPB::from_filter(filter.as_ref()));
          },
          FieldType::Checklist => {
            self.cell_filter_cache.write().insert(
              &filter_type,
              ChecklistFilterPB::from_filter(filter.as_ref()),
            );
          },
        }
      }
    }
  }
}

/// Returns None if there is no change in this row after applying the filter
#[tracing::instrument(level = "trace", skip_all)]
fn filter_row(
  row: &Row,
  result_by_row_id: &DashMap<RowId, FilterResult>,
  field_by_field_id: &HashMap<String, Arc<Field>>,
  cell_data_cache: &AtomicCellDataCache,
  cell_filter_cache: &AtomicCellFilterCache,
) -> Option<(String, bool)> {
  // Create a filter result cache if it's not exist
  let mut filter_result = result_by_row_id
    .entry(row.id.clone())
    .or_insert_with(FilterResult::default);
  let old_is_visible = filter_result.is_visible();

  // Iterate each cell of the row to check its visibility
  for (field_id, field) in field_by_field_id {
    let filter_type = FilterType::from(field.as_ref());
    if !cell_filter_cache.read().contains(&filter_type) {
      filter_result.visible_by_filter_id.remove(&filter_type);
      continue;
    }

    let cell = row.cells.get(field_id).cloned();
    // if the visibility of the cell_rew is changed, which means the visibility of the
    // row is changed too.
    if let Some(is_visible) = filter_cell(
      &filter_type,
      field,
      cell,
      cell_data_cache,
      cell_filter_cache,
    ) {
      filter_result
        .visible_by_filter_id
        .insert(filter_type, is_visible);
    }
  }

  let is_visible = filter_result.is_visible();
  if old_is_visible != is_visible {
    Some((row.id.clone(), is_visible))
  } else {
    None
  }
}

// Returns None if there is no change in this cell after applying the filter
// Returns Some if the visibility of the cell is changed

#[tracing::instrument(level = "trace", skip_all, fields(cell_content))]
fn filter_cell(
  filter_type: &FilterType,
  field: &Arc<Field>,
  cell: Option<Cell>,
  cell_data_cache: &AtomicCellDataCache,
  cell_filter_cache: &AtomicCellFilterCache,
) -> Option<bool> {
  let handler = TypeOptionCellExt::new(
    field.as_ref(),
    Some(cell_data_cache.clone()),
    Some(cell_filter_cache.clone()),
  )
  .get_type_option_cell_data_handler(&filter_type.field_type)?;
  let is_visible =
    handler.handle_cell_filter(filter_type, field.as_ref(), &cell.unwrap_or_default());
  Some(is_visible)
}

#[derive(Serialize, Deserialize, Clone, Debug)]
enum FilterEvent {
  FilterDidChanged,
  RowDidChanged(String),
}

impl ToString for FilterEvent {
  fn to_string(&self) -> String {
    serde_json::to_string(self).unwrap()
  }
}

impl FromStr for FilterEvent {
  type Err = serde_json::Error;
  fn from_str(s: &str) -> Result<Self, Self::Err> {
    serde_json::from_str(s)
  }
}