mod column;
mod differ_database;
mod enums;
mod index;
mod sql_schema_differ_flavour;
mod table;

pub(crate) use column::{ColumnChange, ColumnChanges};
pub(crate) use sql_schema_differ_flavour::SqlSchemaDifferFlavour;

use self::differ_database::DifferDatabase;
use crate::{
    pair::Pair,
    sql_migration::{
        self, AddColumn, AddForeignKey, AlterColumn, AlterEnum, AlterTable, CreateIndex, CreateTable, DropColumn,
        DropForeignKey, DropIndex, DropTable, RedefineTable, SqlMigrationStep, TableChange,
    },
    SqlFlavour, SqlSchema,
};
use column::ColumnTypeChange;
use enums::EnumDiffer;
use sql_schema_describer::{
    walkers::{EnumWalker, ForeignKeyWalker, TableWalker},
    ColumnTypeFamily,
};
use std::collections::HashSet;
use table::TableDiffer;

pub(crate) fn calculate_steps(schemas: Pair<&SqlSchema>, flavour: &dyn SqlFlavour) -> Vec<SqlMigrationStep> {
    let db = DifferDatabase::new(schemas, flavour);

    let mut alter_indexes = alter_indexes(&db, flavour);

    let redefine_indexes = if flavour.can_alter_index() {
        Vec::new()
    } else {
        std::mem::replace(&mut alter_indexes, Vec::new())
    };

    let (drop_tables, mut drop_foreign_keys) = drop_tables(db);
    push_drop_foreign_keys(db, &mut drop_foreign_keys);

    let mut drop_indexes = drop_indexes(db, flavour);
    let mut create_indexes = create_indexes(db);

    let alter_tables = db.alter_tables().collect::<Vec<_>>();

    flavour.push_index_changes_for_column_changes(&alter_tables, &mut drop_indexes, &mut create_indexes, &db);

    let redefine_tables = redefine_tables(db);
    let add_foreign_keys = add_foreign_keys(db);
    let mut alter_enums = flavour.alter_enums(&db);
    push_previous_usages_as_defaults_in_altered_enums(&db, &mut alter_enums);

    let redefine_tables = Some(redefine_tables)
        .filter(|tables| !tables.is_empty())
        .map(SqlMigrationStep::RedefineTables);

    flavour
        .create_enums(&db)
        .into_iter()
        .map(SqlMigrationStep::CreateEnum)
        .chain(alter_enums.into_iter().map(SqlMigrationStep::AlterEnum))
        .chain(drop_foreign_keys.into_iter().map(SqlMigrationStep::DropForeignKey))
        .chain(drop_indexes.into_iter().map(SqlMigrationStep::DropIndex))
        .chain(alter_tables.into_iter().map(SqlMigrationStep::AlterTable))
        // Order matters: we must drop tables before we create indexes,
        // because on Postgres and SQLite, we may create indexes whose names
        // clash with the names of indexes on the dropped tables.
        .chain(drop_tables.into_iter().map(SqlMigrationStep::DropTable))
        // Order matters:
        // - We must drop enums before we create tables, because the new tables
        //   might be named the same as the dropped enum, and that conflicts on
        //   postgres.
        // - We must drop enums after we drop tables, or dropping the enum will
        //   fail on postgres because objects (=tables) still depend on them.
        .chain(flavour.drop_enums(&db).into_iter().map(SqlMigrationStep::DropEnum))
        .chain(db.create_tables().map(SqlMigrationStep::CreateTable))
        .chain(redefine_tables)
        // Order matters: we must create indexes after ALTER TABLEs because the indexes can be
        // on fields that are dropped/created there.
        .chain(create_indexes.into_iter().map(SqlMigrationStep::CreateIndex))
        // Order matters: this needs to come after create_indexes, because the foreign keys can depend on unique
        // indexes created there.
        .chain(add_foreign_keys.into_iter().map(SqlMigrationStep::AddForeignKey))
        .chain(alter_indexes.into_iter().map(|idxs| SqlMigrationStep::AlterIndex {
            table: idxs.as_ref().map(|(table, _)| *table),
            index: idxs.as_ref().map(|(_, idx)| *idx),
        }))
        .chain(
            redefine_indexes
                .into_iter()
                .map(|idxs| SqlMigrationStep::RedefineIndex {
                    table: idxs.as_ref().map(|(table, _)| *table),
                    index: idxs.as_ref().map(|(_, idx)| *idx),
                }),
        )
        .collect()
}

fn create_tables<'a>(db: &'a DifferDatabase<'_>) -> impl Iterator<Item = CreateTable> + 'a {
    db.created_tables().map(|created_table| CreateTable {
        table_index: created_table.table_index(),
    })
}

// We drop the foreign keys of dropped tables first, so we can drop tables in whatever order we
// please later.
fn drop_tables<'a>(db: &'a DifferDatabase<'_>) -> (Vec<DropTable>, Vec<DropForeignKey>) {
    let mut dropped_tables = Vec::with_capacity(db.dropped_tables_count());
    let mut dropped_foreign_keys = Vec::new();

    for dropped_table in db.dropped_tables() {
        dropped_tables.push(DropTable {
            table_index: dropped_table.table_index(),
        });

        for (fk, fk_name) in dropped_table
            .foreign_keys()
            .filter_map(|fk| fk.constraint_name().map(|name| (fk, name)))
        {
            let drop_foreign_key = DropForeignKey {
                table_index: dropped_table.table_index(),
                foreign_key_index: fk.foreign_key_index(),
                table: dropped_table.name().to_owned(),
                constraint_name: fk_name.to_owned(),
            };

            dropped_foreign_keys.push(drop_foreign_key);
        }
    }

    (dropped_tables, dropped_foreign_keys)
}

fn add_foreign_keys(db: &DifferDatabase<'_>, flavour: &dyn SqlFlavour) -> Vec<AddForeignKey> {
    let mut add_foreign_keys = Vec::new();

    if flavour.should_push_foreign_keys_from_created_tables() {
        push_foreign_keys_from_created_tables(&mut add_foreign_keys, db.created_tables());
    }

    push_created_foreign_keys(&mut add_foreign_keys, db.table_pairs());

    add_foreign_keys
}

fn alter_tables<'a>(db: &DifferDatabase<'a>) -> impl Iterator<Item = AlterTable> + 'a {
    db.table_pairs().filter_map(|differ| {
        // Order matters.
        let changes: Vec<TableChange> = drop_primary_key(&differ)
            .into_iter()
            .chain(drop_columns(&differ))
            .chain(add_columns(&differ))
            .chain(alter_columns(&differ))
            .chain(add_primary_key(&differ))
            .collect();

        Some(changes)
            .filter(|changes| !changes.is_empty())
            .map(|changes| AlterTable {
                table_index: differ.tables.map(|t| t.table_index()),
                changes,
            })
    })
}

fn drop_columns<'a>(differ: &'a TableDiffer<'_>) -> impl Iterator<Item = TableChange> + 'a {
    differ.dropped_columns().map(|column| {
        let change = DropColumn {
            index: column.column_index(),
        };

        TableChange::DropColumn(change)
    })
}

fn add_columns<'a>(differ: &'a TableDiffer<'_>) -> impl Iterator<Item = TableChange> + 'a {
    differ.added_columns().map(move |column| {
        let change = AddColumn {
            column_index: column.column_index(),
        };

        TableChange::AddColumn(change)
    })
}

fn alter_columns<'a>(table_differ: &'a TableDiffer<'_>) -> impl Iterator<Item = TableChange> + 'a {
    table_differ.column_pairs().filter_map(move |column_differ| {
        let (changes, type_change) = column_differ.all_changes();

        if !changes.differs_in_something() {
            return None;
        }

        let column_index = Pair::new(column_differ.previous.column_index(), column_differ.next.column_index());

        match type_change {
            Some(ColumnTypeChange::NotCastable) => Some(TableChange::DropAndRecreateColumn { column_index, changes }),
            Some(ColumnTypeChange::RiskyCast) => Some(TableChange::AlterColumn(AlterColumn {
                column_index,
                changes,
                type_change: Some(crate::sql_migration::ColumnTypeChange::RiskyCast),
            })),
            Some(ColumnTypeChange::SafeCast) => Some(TableChange::AlterColumn(AlterColumn {
                column_index,
                changes,
                type_change: Some(crate::sql_migration::ColumnTypeChange::SafeCast),
            })),
            None => Some(TableChange::AlterColumn(AlterColumn {
                column_index,
                changes,
                type_change: None,
            })),
        }
    })
}

fn push_drop_foreign_keys(db: DifferDatabase<'_>, drop_foreign_keys: &mut Vec<DropForeignKey>) {
    for differ in db.table_pairs() {
        for (dropped_fk, dropped_foreign_key_name) in differ
            .dropped_foreign_keys()
            .filter_map(|foreign_key| foreign_key.constraint_name().map(|name| (foreign_key, name)))
        {
            drop_foreign_keys.push(DropForeignKey {
                table_index: differ.previous().table_index(),
                table: differ.previous().name().to_owned(),
                foreign_key_index: dropped_fk.foreign_key_index(),
                constraint_name: dropped_foreign_key_name.to_owned(),
            })
        }
    }
}

fn add_primary_key(differ: &TableDiffer<'_>) -> Option<TableChange> {
    let from_psl_change = differ
        .created_primary_key()
        .filter(|pk| !pk.columns.is_empty())
        .map(|pk| TableChange::AddPrimaryKey {
            columns: pk.columns.clone(),
        });

    if differ.flavour.should_recreate_the_primary_key_on_column_recreate() {
        from_psl_change.or_else(|| {
            let from_recreate = Self::alter_columns(differ).any(|tc| match tc {
                TableChange::DropAndRecreateColumn { column_index, .. } => {
                    let idx = *column_index.previous();
                    differ.previous().column_at(idx).is_part_of_primary_key()
                }
                _ => false,
            });

            if from_recreate {
                Some(TableChange::AddPrimaryKey {
                    columns: differ.previous().table().primary_key_columns(),
                })
            } else {
                None
            }
        })
    } else {
        from_psl_change
    }
}

fn drop_primary_key(differ: &TableDiffer<'_>) -> Option<TableChange> {
    let from_psl_change = differ.dropped_primary_key().map(|_pk| TableChange::DropPrimaryKey);

    if differ.flavour.should_recreate_the_primary_key_on_column_recreate() {
        from_psl_change.or_else(|| {
            let from_recreate = Self::alter_columns(differ).any(|tc| match tc {
                TableChange::DropAndRecreateColumn { column_index, .. } => {
                    let idx = *column_index.previous();
                    differ.previous().column_at(idx).is_part_of_primary_key()
                }
                _ => false,
            });

            if from_recreate {
                Some(TableChange::DropPrimaryKey)
            } else {
                None
            }
        })
    } else {
        from_psl_change
    }
}

fn create_indexes(db: DifferDatabase<'_>, flavour: &dyn SqlFlavour) -> Vec<CreateIndex> {
    let mut steps = Vec::new();

    if flavour.should_create_indexes_from_created_tables() {
        let create_indexes_from_created_tables = self
            .created_tables()
            .flat_map(|table| table.indexes())
            .filter(|index| !self.flavour.should_skip_index_for_new_table(index))
            .map(|index| CreateIndex {
                table_index: index.table().table_index(),
                index_index: index.index(),
                caused_by_create_table: true,
            });

        steps.extend(create_indexes_from_created_tables);
    }

    for tables in db.table_pairs() {
        for index in tables.created_indexes() {
            steps.push(CreateIndex {
                table_index: index.table().table_index(),
                index_index: index.index(),
                caused_by_create_table: false,
            })
        }

        if flavour.indexes_should_be_recreated_after_column_drop() {
            let dropped_and_recreated_column_indexes_next: HashSet<usize> = tables
                .column_pairs()
                .filter(|columns| matches!(columns.all_changes().1, Some(ColumnTypeChange::NotCastable)))
                .map(|col| col.as_pair().next().column_index())
                .collect();

            for index in tables.index_pairs().filter(|index| {
                index
                    .next()
                    .columns()
                    .any(|col| dropped_and_recreated_column_indexes_next.contains(&col.column_index()))
            }) {
                steps.push(CreateIndex {
                    table_index: tables.next().table_index(),
                    index_index: index.next().index(),
                    caused_by_create_table: false,
                })
            }
        }
    }

    steps
}

fn drop_indexes(db: &DifferDatabase<'_>, flavour: &dyn SqlFlavour) -> Vec<DropIndex> {
    let mut drop_indexes = HashSet::new();

    for tables in db.table_pairs() {
        for index in tables.dropped_indexes() {
            // On MySQL, foreign keys automatically create indexes. These foreign-key-created
            // indexes should only be dropped as part of the foreign key.
            if flavour.should_skip_fk_indexes() && index::index_covers_fk(&tables.previous(), &index) {
                continue;
            }

            drop_indexes.insert(DropIndex {
                table_index: index.table().table_index(),
                index_index: index.index(),
            });
        }
    }

    // On SQLite, we will recreate indexes in the RedefineTables step,
    // because they are needed for implementing new foreign key constraints.
    if !tables_to_redefine.is_empty() && self.flavour.should_drop_indexes_from_dropped_tables() {
        for table in self.dropped_tables() {
            for index in table.indexes() {
                drop_indexes.insert(DropIndex {
                    table_index: index.table().table_index(),
                    index_index: index.index(),
                });
            }
        }
    }

    drop_indexes.into_iter().collect()
}

fn redefine_tables(db: DifferDatabase<'_>) -> Vec<RedefineTable> {
    db.tables_to_redefine()
        .map(|differ| {
            let column_pairs = differ
                .column_pairs()
                .map(|columns| {
                    let (changes, type_change) = columns.all_changes();
                    (
                        Pair::new(columns.previous.column_index(), columns.next.column_index()),
                        changes,
                        type_change.map(|tc| match tc {
                            ColumnTypeChange::SafeCast => sql_migration::ColumnTypeChange::SafeCast,
                            ColumnTypeChange::RiskyCast => sql_migration::ColumnTypeChange::RiskyCast,
                            ColumnTypeChange::NotCastable => sql_migration::ColumnTypeChange::NotCastable,
                        }),
                    )
                })
                .collect();

            RedefineTable {
                table_index: differ.tables.as_ref().map(|t| t.table_index()),
                dropped_primary_key: drop_primary_key(&differ).is_some(),
                added_columns: differ.added_columns().map(|col| col.column_index()).collect(),
                dropped_columns: differ.dropped_columns().map(|col| col.column_index()).collect(),
                column_pairs,
            }
        })
        .collect()
}

fn alter_indexes(db: &DifferDatabase<'_>, flavour: &dyn SqlFlavour) -> Vec<Pair<(usize, usize)>> {
    let mut steps = Vec::new();

    for differ in db.table_pairs() {
        for pair in differ
            .index_pairs()
            .filter(|pair| flavour.index_should_be_renamed(&pair))
        {
            steps.push(pair.as_ref().map(|i| (i.table().table_index(), i.index())));
        }
    }

    steps
}

fn created_tables(&self) -> impl Iterator<Item = TableWalker<'_>> {
    self.next_tables().filter(move |next_table| {
        !self.previous_tables().any(|previous_table| {
            self.flavour
                .table_names_match(Pair::new(previous_table.name(), next_table.name()))
        })
    })
}

fn table_is_ignored(&self, table_name: &str) -> bool {
    table_name == "_prisma_migrations" || self.flavour.table_should_be_ignored(&table_name)
}

fn enum_pairs(&self) -> impl Iterator<Item = EnumDiffer<'_>> {
    self.previous_enums().filter_map(move |previous| {
        self.next_enums()
            .find(|next| enums_match(&previous, &next))
            .map(|next| EnumDiffer {
                enums: Pair::new(previous, next),
            })
    })
}

fn created_enums<'a>(&'a self) -> impl Iterator<Item = EnumWalker<'schema>> + 'a {
    self.next_enums()
        .filter(move |next| !self.previous_enums().any(|previous| enums_match(&previous, next)))
}

fn dropped_enums<'a>(&'a self) -> impl Iterator<Item = EnumWalker<'schema>> + 'a {
    self.previous_enums()
        .filter(move |previous| !self.next_enums().any(|next| enums_match(previous, &next)))
}

fn push_previous_usages_as_defaults_in_altered_enums(differ: &SqlSchemaDiffer<'_>, alter_enums: &mut [AlterEnum]) {
    for alter_enum in alter_enums {
        let mut previous_usages_as_default = Vec::new();

        let enum_names = differ.schemas.enums(&alter_enum.index).map(|enm| enm.name());

        for table in differ.dropped_tables() {
            for column in table
                .columns()
                .filter(|col| col.column_type_is_enum(enum_names.previous()) && col.default().is_some())
            {
                previous_usages_as_default.push(((column.table().table_index(), column.column_index()), None));
            }
        }

        for tables in differ.table_pairs() {
            for column in tables
                .dropped_columns()
                .filter(|col| col.column_type_is_enum(enum_names.previous()) && col.default().is_some())
            {
                previous_usages_as_default.push(((column.table().table_index(), column.column_index()), None));
            }

            for columns in tables.column_pairs().filter(|col| {
                col.previous.column_type_is_enum(enum_names.previous()) && col.previous.default().is_some()
            }) {
                let next_usage_as_default = Some(&columns.next)
                    .filter(|col| col.column_type_is_enum(enum_names.next()) && col.default().is_some())
                    .map(|col| (col.table().table_index(), col.column_index()));

                previous_usages_as_default.push((
                    (columns.previous.table().table_index(), columns.previous.column_index()),
                    next_usage_as_default,
                ));
            }
        }

        alter_enum.previous_usages_as_default = previous_usages_as_default;
    }
}

fn push_created_foreign_keys<'a, 'schema>(
    added_foreign_keys: &mut Vec<AddForeignKey>,
    table_pairs: impl Iterator<Item = TableDiffer<'schema>>,
) {
    table_pairs.for_each(|differ| {
        added_foreign_keys.extend(differ.created_foreign_keys().map(|created_fk| AddForeignKey {
            table_index: differ.next().table_index(),
            foreign_key_index: created_fk.foreign_key_index(),
        }))
    })
}

fn push_foreign_keys_from_created_tables<'a>(
    steps: &mut Vec<AddForeignKey>,
    created_tables: impl Iterator<Item = TableWalker<'a>>,
) {
    for table in created_tables {
        steps.extend(table.foreign_keys().map(|fk| AddForeignKey {
            table_index: table.table_index(),
            foreign_key_index: fk.foreign_key_index(),
        }));
    }
}

/// Compare two [ForeignKey](/sql-schema-describer/struct.ForeignKey.html)s and return whether they
/// should be considered equivalent for schema diffing purposes.
fn foreign_keys_match(fks: Pair<&ForeignKeyWalker<'_>>, flavour: &dyn SqlFlavour) -> bool {
    let references_same_table = flavour.table_names_match(fks.map(|fk| fk.referenced_table().name()));
    let references_same_column_count =
        fks.previous().referenced_columns_count() == fks.next().referenced_columns_count();
    let constrains_same_column_count =
        fks.previous().constrained_columns().count() == fks.next().constrained_columns().count();
    let constrains_same_columns = fks.interleave(|fk| fk.constrained_columns()).all(|fks| {
        let families_match = match fks.map(|fk| fk.column_type_family()).as_tuple() {
            (ColumnTypeFamily::Uuid, ColumnTypeFamily::String) => true,
            (ColumnTypeFamily::String, ColumnTypeFamily::Uuid) => true,
            (x, y) => x == y,
        };

        fks.previous().name() == fks.next().name() && families_match
    });

    // Foreign key references different columns or the same columns in a different order.
    let references_same_columns = fks
        .interleave(|fk| fk.referenced_column_names())
        .all(|pair| pair.previous() == pair.next());

    references_same_table
        && references_same_column_count
        && constrains_same_column_count
        && constrains_same_columns
        && references_same_columns
}

fn enums_match(previous: &EnumWalker<'_>, next: &EnumWalker<'_>) -> bool {
    previous.name() == next.name()
}
