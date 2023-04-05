use crate::entities::{
  FieldType, GroupRowsNotificationPB, InsertedRowPB, RowPB, SelectOptionCellDataPB,
};
use crate::services::cell::{insert_checkbox_cell, insert_select_option_cell, insert_url_cell};
use crate::services::field::{SelectOption, CHECK};

use crate::services::group::controller::MoveGroupRowContext;
use crate::services::group::{GeneratedGroupConfig, GroupData};
use collab_database::fields::Field;
use collab_database::rows::{Cell, Row};
use collab_database::views::Group;

pub fn add_or_remove_select_option_row(
  group: &mut GroupData,
  cell_data: &SelectOptionCellDataPB,
  row: &Row,
) -> Option<GroupRowsNotificationPB> {
  let mut changeset = GroupRowsNotificationPB::new(group.id.clone());
  if cell_data.select_options.is_empty() {
    if group.contains_row(&row.id) {
      changeset.deleted_rows.push(row.id.clone());
      group.remove_row(&row.id);
    }
  } else {
    cell_data.select_options.iter().for_each(|option| {
      if option.id == group.id {
        if !group.contains_row(&row.id) {
          let row_pb = RowPB::from(row);
          changeset
            .inserted_rows
            .push(InsertedRowPB::new(row_pb.clone()));
          group.add_row(row_pb);
        }
      } else if group.contains_row(&row.id) {
        changeset.deleted_rows.push(row.id.clone());
        group.remove_row(&row.id);
      }
    });
  }

  if changeset.is_empty() {
    None
  } else {
    Some(changeset)
  }
}

pub fn remove_select_option_row(
  group: &mut GroupData,
  cell_data: &SelectOptionCellDataPB,
  row: &Row,
) -> Option<GroupRowsNotificationPB> {
  let mut changeset = GroupRowsNotificationPB::new(group.id.clone());
  cell_data.select_options.iter().for_each(|option| {
    if option.id == group.id && group.contains_row(&row.id) {
      changeset.deleted_rows.push(row.id.clone());
      group.remove_row(&row.id);
    }
  });

  if changeset.is_empty() {
    None
  } else {
    Some(changeset)
  }
}

pub fn move_group_row(
  group: &mut GroupData,
  context: &mut MoveGroupRowContext,
) -> Option<GroupRowsNotificationPB> {
  let mut changeset = GroupRowsNotificationPB::new(group.id.clone());
  let MoveGroupRowContext {
    row,
    row_changeset,
    field,
    to_group_id,
    to_row_id,
  } = context;

  let from_index = group.index_of_row(&row.id);
  let to_index = match to_row_id {
    None => None,
    Some(to_row_id) => group.index_of_row(to_row_id),
  };

  // Remove the row in which group contains it
  if let Some(from_index) = &from_index {
    changeset.deleted_rows.push(row.id.clone());
    tracing::debug!("Group:{} remove {} at {}", group.id, row.id, from_index);
    group.remove_row(&row.id);
  }

  if group.id == *to_group_id {
    let row_pb = RowPB::from(*row);
    let mut inserted_row = InsertedRowPB::new(row_pb.clone());
    match to_index {
      None => {
        changeset.inserted_rows.push(inserted_row);
        tracing::debug!("Group:{} append row:{}", group.id, row.id);
        group.add_row(row_pb);
      },
      Some(to_index) => {
        if to_index < group.number_of_row() {
          tracing::debug!("Group:{} insert {} at {} ", group.id, row.id, to_index);
          inserted_row.index = Some(to_index as i32);
          group.insert_row(to_index, row_pb);
        } else {
          tracing::warn!("Move to index: {} is out of bounds", to_index);
          tracing::debug!("Group:{} append row:{}", group.id, row.id);
          group.add_row(row_pb);
        }
        changeset.inserted_rows.push(inserted_row);
      },
    }

    // Update the corresponding row's cell content.
    // If the from_index is none which means the row is not belong to this group before and
    // it is moved from other groups.
    if from_index.is_none() {
      let cell = make_inserted_cell_rev(&group.id, field);
      if let Some(cell) = cell {
        tracing::debug!(
          "Update content of the cell in the row:{} to group:{}",
          row.id,
          group.id
        );
        row_changeset
          .cell_by_field_id
          .insert(field.id.clone(), cell);
      }
    }
  }
  if changeset.is_empty() {
    None
  } else {
    Some(changeset)
  }
}

pub fn make_inserted_cell_rev(group_id: &str, field: &Field) -> Option<Cell> {
  let field_type = FieldType::from(field.field_type);
  match field_type {
    FieldType::SingleSelect => {
      let cell = insert_select_option_cell(vec![group_id.to_owned()], field);
      Some(cell)
    },
    FieldType::MultiSelect => {
      let cell = insert_select_option_cell(vec![group_id.to_owned()], field);
      Some(cell)
    },
    FieldType::Checkbox => {
      let cell = insert_checkbox_cell(group_id == CHECK, field);
      Some(cell)
    },
    FieldType::URL => {
      let cell = insert_url_cell(group_id.to_owned(), field);
      Some(cell)
    },
    _ => {
      tracing::warn!("Unknown field type: {:?}", field_type);
      None
    },
  }
}
pub fn generate_select_option_groups(
  _field_id: &str,
  options: &[SelectOption],
) -> Vec<GeneratedGroupConfig> {
  let groups = options
    .iter()
    .map(|option| GeneratedGroupConfig {
      group: Group::new(option.id.clone(), option.name.clone()),
      filter_content: option.id.clone(),
    })
    .collect();

  groups
}