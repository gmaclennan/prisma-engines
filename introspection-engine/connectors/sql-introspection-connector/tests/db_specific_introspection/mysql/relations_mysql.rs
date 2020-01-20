use crate::*;
use barrel::types;
use test_harness::*;

#[test_one_connector(connector = "mysql")]
async fn introspecting_a_one_to_one_req_relation_should_work(api: &TestApi) {
    let barrel = api.barrel();
    let _setup_schema = barrel
        .execute_with_schema(
            |migration| {
                migration.create_table("User", |t| {
                    t.add_column("id", types::primary());
                });
                migration.create_table("Post", |t| {
                    t.add_column("id", types::primary());
                    t.inject_custom(
                        "user_id INTEGER NOT NULL UNIQUE,
                FOREIGN KEY (`user_id`) REFERENCES `User`(`id`)",
                    )
                });
            },
            api.db_name(),
        )
        .await;

    let dm = r#"
              model Post {
               id      Int @id
               user_id User
            }
          
            model User {
               id      Int @id
               post Post?
            }
        "#;
    let result = dbg!(api.introspect().await);
    custom_assert(&result, dm);
}

#[test_one_connector(connector = "mysql")]
async fn introspecting_two_one_to_one_relations_between_the_same_models_should_work(api: &TestApi) {
    let barrel = api.barrel();
    let _setup_schema = barrel
        .execute_with_schema(
            |migration| {
                migration.create_table("User", |t| {
                    t.add_column("id", types::primary());
                });
                migration.create_table("Post", |t| {
                    t.add_column("id", types::primary());
                    t.inject_custom(
                        "user_id INTEGER NOT NULL UNIQUE,\
                         FOREIGN KEY(`user_id`) REFERENCES `User`(`id`)",
                    )
                });
            },
            api.db_name(),
        )
        .await;

    api.database()
        .execute_raw(
            &format!(
                "ALTER TABLE `{}`.`User` ADD Column `post_id` INTEGER NOT NULL UNIQUE ",
                api.db_name(),
            ),
            &[],
        )
        .await
        .unwrap();

    api.database()
        .execute_raw(
            &format!(
                "ALTER TABLE `{}`.`User` ADD CONSTRAINT `post_fk` FOREIGN KEY(`post_id`) REFERENCES `Post`(`id`)",
                api.db_name(),
            ),
            &[],
        )
        .await
        .unwrap();

    let dm = r#"
            model Post {
               id      Int @id
               user_id User  @relation("Post_user_idToUser")
               user    User? @relation("PostToUser_post_id", references: [post_id])
            }
        
            model User {
               id      Int @id
               post_id Post  @relation("PostToUser_post_id")
               post Post?    @relation("Post_user_idToUser")
            }
        "#;
    let result = dbg!(api.introspect().await);
    custom_assert(&result, dm);
}

#[test_one_connector(connector = "mysql")]
async fn introspecting_a_one_to_one_relation_should_work(api: &TestApi) {
    let barrel = api.barrel();
    let _setup_schema = barrel
        .execute_with_schema(
            |migration| {
                migration.create_table("User", |t| {
                    t.add_column("id", types::primary());
                });
                migration.create_table("Post", |t| {
                    t.add_column("id", types::primary());
                    t.inject_custom(
                        "user_id INTEGER UNIQUE,\
                         FOREIGN KEY (`user_id`) REFERENCES `User`(`id`)",
                    );
                });
            },
            api.db_name(),
        )
        .await;

    let dm = r#"        
            model Post {
               id      Int @id
               user_id User?
            }
            
            model User {
               id      Int @id
               post Post? 
              
            }
        "#;
    let result = dbg!(api.introspect().await);
    custom_assert(&result, dm);
}

#[test_one_connector(connector = "mysql")]
async fn introspecting_a_one_to_one_relation_referencing_non_id_should_work(api: &TestApi) {
    let barrel = api.barrel();
    let _setup_schema = barrel
        .execute(|migration| {
            migration.create_table("User", |t| {
                t.add_column("id", types::primary());
                t.inject_custom("email varchar(10) UNIQUE");
            });
            migration.create_table("Post", |t| {
                t.add_column("id", types::primary());
                t.inject_custom(
                    "user_email varchar(10) UNIQUE,
                FOREIGN KEY (`user_email`) REFERENCES `User`(`email`)",
                );
            });
        })
        .await;
    let dm = r#"        
            model Post {
               id           Int     @id  
               user_email   User?   @relation(references: [email])
            }
            
            model User {
               email        String? @unique 
               id           Int     @id 
               post         Post? 
            }
        "#;
    let result = dbg!(api.introspect().await);
    custom_assert(&result, dm);
}

#[test_one_connector(connector = "mysql")]
async fn introspecting_a_one_to_many_relation_should_work(api: &TestApi) {
    let barrel = api.barrel();
    let _setup_schema = barrel
        .execute_with_schema(
            |migration| {
                migration.create_table("User", |t| {
                    t.add_column("id", types::primary());
                });
                migration.create_table("Post", |t| {
                    t.add_column("id", types::primary());
                    t.inject_custom(
                        "user_id INTEGER,\
                         FOREIGN KEY (`user_id`) REFERENCES `User`(`id`)",
                    );
                });
            },
            api.db_name(),
        )
        .await;

    let dm = r#"  
            model Post {
               id      Int @id
               user_id User?
               
               @@index([user_id], name: "user_id")
            }
            
            model User {
               id      Int @id
               posts Post[] 
            }
        "#;
    let result = dbg!(api.introspect().await);
    custom_assert(&result, dm);
}

#[test_one_connector(connector = "mysql")]
async fn introspecting_a_one_req_to_many_relation_should_work(api: &TestApi) {
    let barrel = api.barrel();
    let _setup_schema = barrel
        .execute_with_schema(
            |migration| {
                migration.create_table("User", |t| {
                    t.add_column("id", types::primary());
                });
                migration.create_table("Post", |t| {
                    t.add_column("id", types::primary());
                    t.inject_custom(
                        "user_id INTEGER NOT NULL,\
                         FOREIGN KEY (`user_id`) REFERENCES `User`(`id`)",
                    );
                });
            },
            api.db_name(),
        )
        .await;

    let dm = r#"
            model Post {
               id      Int @id
               user_id User
               
               @@index([user_id], name: "user_id")
            }
            
            model User {
               id      Int @id
               posts Post[] 
            }
       "#;
    let result = dbg!(api.introspect().await);
    custom_assert(&result, dm);
}

#[test_one_connector(connector = "mysql")]
async fn introspecting_a_prisma_many_to_many_relation_should_work(api: &TestApi) {
    let barrel = api.barrel();
    let _setup_schema = barrel
        .execute_with_schema(
            |migration| {
                migration.create_table("User", |t| {
                    t.add_column("id", types::primary());
                });
                migration.create_table("Post", |t| {
                    t.add_column("id", types::primary());
                });
                migration.create_table("_PostToUser", |t| {
                    t.inject_custom(
                        "A INTEGER NOT NULL,
                     B INTEGER NOT NULL,
                     FOREIGN KEY (`A`) REFERENCES  `Post`(`id`) ON DELETE CASCADE,
                     FOREIGN KEY (`B`) REFERENCES  `User`(`id`) ON DELETE CASCADE",
                    )
                });
            },
            api.db_name(),
        )
        .await;

    api.database()
        .execute_raw(
            &format!(
                "CREATE UNIQUE INDEX test ON `{schema_name}`.`_PostToUser` (`A`, `B`);",
                schema_name = api.db_name()
            ),
            &[],
        )
        .await
        .unwrap();

    let dm = r#"
            model Post {
               id      Int @id
               users User[] 
            }
            
            model User {
               id      Int @id
               posts Post[] 
            }
        "#;
    let result = dbg!(api.introspect().await);
    custom_assert(&result, dm);
}

#[test_one_connector(connector = "mysql")]
async fn introspecting_a_many_to_many_relation_should_work(api: &TestApi) {
    let barrel = api.barrel();
    let _setup_schema = barrel
        .execute_with_schema(
            |migration| {
                migration.create_table("User", |t| {
                    t.add_column("id", types::primary());
                });
                migration.create_table("Post", |t| {
                    t.add_column("id", types::primary());
                });
                migration.create_table("PostsToUsers", |t| {
                    t.inject_custom(
                        "user_id INTEGER NOT NULL,
                     post_id INTEGER NOT NULL,
                     FOREIGN KEY (`user_id`) REFERENCES  `User`(`id`) ON DELETE CASCADE,
                     FOREIGN KEY (`post_id`) REFERENCES  `Post`(`id`) ON DELETE CASCADE",
                    )
                });
            },
            api.db_name(),
        )
        .await;

    let dm = r#"
            model Post {
               id      Int @id
               postsToUserses PostsToUsers[] @relation(references: [post_id], onDelete: CASCADE)
            }

            model PostsToUsers {
              post_id Post 
              user_id User
              
              @@index([post_id], name: "post_id")  
              @@index([user_id], name: "user_id")
            }
            
            model User {
               id      Int @id
               postsToUserses PostsToUsers[] @relation(onDelete: CASCADE)
            }
        "#;
    let result = dbg!(api.introspect().await);
    custom_assert(&result, dm);
}

#[test_one_connector(connector = "mysql")]
async fn introspecting_a_many_to_many_relation_with_extra_fields_should_work(api: &TestApi) {
    let barrel = api.barrel();
    let _setup_schema = barrel
        .execute_with_schema(
            |migration| {
                migration.create_table("User", |t| {
                    t.add_column("id", types::primary());
                });
                migration.create_table("Post", |t| {
                    t.add_column("id", types::primary());
                });
                migration.create_table("PostsToUsers", |t| {
                    t.inject_custom(
                        "date    date,
                     user_id INTEGER NOT NULL,
                     post_id INTEGER NOT NULL,
                     FOREIGN KEY (`user_id`) REFERENCES  `User`(`id`),
                     FOREIGN KEY (`post_id`) REFERENCES  `Post`(`id`)",
                    )
                });
            },
            api.db_name(),
        )
        .await;

    let dm = r#"
            model Post {
               id      Int @id
               postsToUserses PostsToUsers[] @relation(references: [post_id])
            }
            
            model PostsToUsers {
              date    DateTime?
              post_id Post 
              user_id User
              
              @@index([post_id], name: "post_id")  
              @@index([user_id], name: "user_id")
            }
            
            model User {
               id      Int @id
               postsToUserses PostsToUsers[] 
            }
        "#;
    let result = dbg!(api.introspect().await);
    custom_assert(&result, dm);
}
#[test_one_connector(connector = "mysql")]
async fn introspecting_a_self_relation_should_work(api: &TestApi) {
    let barrel = api.barrel();
    let _setup_schema = barrel
        .execute_with_schema(
            |migration| {
                migration.create_table("User", |t| {
                    t.add_column("id", types::primary());
                    t.inject_custom(
                        "recruited_by INTEGER, 
                     direct_report INTEGER,
                     FOREIGN KEY (`recruited_by`) REFERENCES `User` (`id`),
                     FOREIGN KEY (`direct_report`) REFERENCES `User` (`id`)",
                    )
                });
            },
            api.db_name(),
        )
        .await;

    let dm = r#"
            model User {
                id                             Int @id
                direct_report                  User?  @relation("UserToUser_direct_report")
                recruited_by                   User?  @relation("UserToUser_recruited_by")
                users_UserToUser_direct_report User[] @relation("UserToUser_direct_report")
                users_UserToUser_recruited_by  User[] @relation("UserToUser_recruited_by")
                
                @@index([direct_report], name: "direct_report")  
                @@index([recruited_by], name: "recruited_by")
            }
        "#;
    let result = dbg!(api.introspect().await);
    custom_assert(&result, dm);
}

// on delete cascade

#[test_one_connector(connector = "mysql")]
async fn introspecting_cascading_delete_behaviour_should_work(api: &TestApi) {
    let barrel = api.barrel();
    let _setup_schema = barrel
        .execute_with_schema(
            |migration| {
                migration.create_table("User", |t| {
                    t.add_column("id", types::primary());
                });
                migration.create_table("Post", |t| {
                    t.add_column("id", types::primary());
                    t.inject_custom(
                        "user_id INTEGER, FOREIGN KEY (`user_id`) REFERENCES `User`(`id`) ON DELETE CASCADE",
                    );
                });
            },
            api.db_name(),
        )
        .await;

    let dm = r#"  
            model Post {
               id      Int @id
               user_id User?
                 
               @@index([user_id], name: "user_id")
            }
            
            model User {
               id      Int @id
               posts Post[] @relation(onDelete: CASCADE)
            }
        "#;
    let result = api.introspect().await;
    custom_assert(&result, dm);
}

#[test_one_connector(connector = "mysql")]
async fn introspecting_id_fields_with_foreign_key_should_ignore_the_relation(api: &TestApi) {
    let barrel = api.barrel();
    let _setup_schema = barrel
        .execute(|migration| {
            migration.create_table("User", |t| {
                t.add_column("id", types::primary());
            });
            migration.create_table("Post", |t| {
                t.add_column("test", types::text());
                t.inject_custom("user_id INTEGER Primary Key");
                t.inject_custom("FOREIGN KEY (`user_id`) REFERENCES `User`(`id`)");
            });
        })
        .await;

    let dm = r#"
            model Post {
               test    String
               user_id Int @id
            }

            model User {
               id      Int @id
            }
        "#;
    let result = dbg!(api.introspect().await);
    custom_assert(&result, dm);
}