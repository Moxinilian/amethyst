use amethyst_core::{
    ecs::prelude::{
        BitSet, ComponentEvent, Join, ReadStorage, ReaderId, Resources, System, WriteStorage,
    },
    math::{convert, Matrix4},
    Transform,
};
use amethyst_rendy::skinning::JointTransforms;

use log::error;

#[cfg(feature = "profiler")]
use thread_profiler::profile_scope;

use super::resources::*;

/// System for performing vertex skinning.
///
/// Needs to run after global transforms have been updated for the current frame.
#[derive(Default, Debug)]
pub struct VertexSkinningSystem {
    /// Also scratch space, used while determining which skins need to be updated.
    updated: BitSet,
    updated_skins: BitSet,
    /// Used for tracking modifications to global transforms
    updated_id: Option<ReaderId<ComponentEvent>>,
}

impl VertexSkinningSystem {
    /// Creates a new `VertexSkinningSystem`
    pub fn new() -> Self {
        Self::default()
    }
}

impl<'a> System<'a> for VertexSkinningSystem {
    type SystemData = (
        ReadStorage<'a, Joint>,
        ReadStorage<'a, Transform>,
        WriteStorage<'a, Skin>,
        WriteStorage<'a, JointTransforms>,
    );

    fn run(&mut self, (joints, global_transforms, mut skins, mut matrices): Self::SystemData) {
        #[cfg(feature = "profiler")]
        profile_scope!("vertex_skinning_system");

        self.updated.clear();

        global_transforms
            .channel()
            .read(self.updated_id.as_mut().expect(
                "`VertexSkinningSystem::setup` was not called before `VertexSkinningSystem::run`",
            ))
            .for_each(|event| match event {
                ComponentEvent::Inserted(id) | ComponentEvent::Modified(id) => {
                    self.updated.add(*id);
                }
                ComponentEvent::Removed(_id) => {}
            });

        self.updated_skins.clear();

        for (_, joint) in (&self.updated, &joints).join() {
            for skin in &joint.skins {
                self.updated_skins.add(skin.id());
            }
        }

        for (_id, skin) in (&self.updated_skins, &mut skins).join() {
            // Compute the joint global_transforms
            skin.joint_matrices.clear();
            let bind_shape = skin.bind_shape_matrix;
            skin.joint_matrices.extend(
                skin.joints
                    .iter()
                    .zip(skin.inverse_bind_matrices.iter())
                    .map(|(joint_entity, inverse_bind_matrix)| {
                        if let Some(transform) = global_transforms.get(*joint_entity) {
                            Some((transform, inverse_bind_matrix))
                        } else {
                            error!(
                                "Missing `Transform` Component for join entity {:?}",
                                joint_entity
                            );
                            None
                        }
                    })
                    .flatten()
                    .map(|(global, inverse_bind_matrix)| {
                        (global.global_matrix() * inverse_bind_matrix * bind_shape)
                    }),
            );

            // update the joint matrices in all referenced mesh entities
            for (_, mesh_global, matrix) in (&skin.meshes, &global_transforms, &mut matrices).join()
            {
                if let Some(global_inverse) = mesh_global.global_matrix().try_inverse() {
                    matrix.matrices.clear();
                    matrix
                        .matrices
                        .extend(skin.joint_matrices.iter().map(|joint_matrix| {
                            convert::<_, Matrix4<f32>>(global_inverse * joint_matrix)
                        }));
                }
            }
        }

        for (_, mesh_global, joint_transform) in
            (&self.updated, &global_transforms, &mut matrices).join()
        {
            if let Some(global_inverse) = mesh_global.global_matrix().try_inverse() {
                if let Some(skin) = skins.get(joint_transform.skin) {
                    joint_transform.matrices.clear();
                    joint_transform
                        .matrices
                        .extend(skin.joint_matrices.iter().map(|joint_matrix| {
                            convert::<_, Matrix4<f32>>(global_inverse * joint_matrix)
                        }));
                } else {
                    error!(
                        "Missing `Skin` Component for join transform entity {:?}",
                        joint_transform.skin
                    );
                }
            }
        }
    }

    fn setup(&mut self, res: &mut Resources) {
        use amethyst_core::ecs::prelude::SystemData;
        Self::SystemData::setup(res);
        let mut transform = WriteStorage::<Transform>::fetch(res);
        self.updated_id = Some(transform.register_reader());
    }
}
