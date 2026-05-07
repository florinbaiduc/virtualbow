#include "LimbMesh.hpp"
#include "LayerColors.hpp"
#include "solver/BowModel.hpp"
#include "solver/BowResult.hpp"
#include <QVector3D>
#include <QColor>
#include <array>

LimbMesh::LimbMesh(const BowModel& bow, const LimbInfo& geometry, LimbSide side)
    : faces_right(GL_QUADS),
      faces_left(GL_QUADS)
{
    size_t nLayers = geometry.heights[0].size();
    size_t nSegments = geometry.heights.size() - 1;

    // Transition points between layers at the left and right of the previous and next cross section
    std::vector<QVector3D> points_l_prev(nLayers + 1);
    std::vector<QVector3D> points_l_next(nLayers + 1);
    std::vector<QVector3D> points_r_prev(nLayers + 1);
    std::vector<QVector3D> points_r_next(nLayers + 1);

    const auto& layers = (side == LimbSide::Upper) ? bow.section.upper.layers : bow.section.lower.layers;
    std::vector<QColor> colors(nLayers);
    for(size_t i = 0; i < nLayers; ++i) {
        colors[i] = getLayerColor(*std::next(layers.begin(), i), bow.section.materials);
    }

    // The lower limb's geometry is stored in the world frame after an
    // x-mirror that maps (x, y, φ) → (-x, y, π−φ). The mirror preserves the
    // y-component of the cross-section normal, but the formula
    //   normal_h = (-sin φ, cos φ)
    // evaluated at the mirrored angle yields the y-component negated. To
    // recover the correct world-frame normal for the lower limb we negate
    // the standard formula. This makes the back/belly of both limbs sit on
    // the same side of the bow so symmetric bows (flatbow, longbow, recurve,
    // …) join continuously at the grip.
    const double normal_sign = (side == LimbSide::Upper) ? 1.0 : -1.0;
    reverse_winding = (side != LimbSide::Upper);

    // Iterate over segments, i.e. pairs of a previous and a next cross section
    for(size_t iSegment = 0; iSegment < nSegments; ++iSegment) {
        std::array<double, 3> profile_prev = geometry.position_eval[iSegment];
        std::array<double, 3> profile_next = geometry.position_eval[iSegment + 1];

        QVector3D center_prev  ( profile_prev[0], profile_prev[1], 0.0 );
        QVector3D normal_w_prev( 0.0, 0.0, 1.0 );
        QVector3D normal_h_prev(normal_sign*-sin(profile_prev[2]), normal_sign*cos(profile_prev[2]), 0.0 );

        QVector3D center_next  ( profile_next[0], profile_next[1], 0.0 );
        QVector3D normal_w_next( 0.0, 0.0, 1.0 );
        QVector3D normal_h_next(normal_sign*-sin(profile_next[2]), normal_sign*cos(profile_next[2]), 0.0 );

        double w_prev = geometry.width[iSegment];
        double w_next = geometry.width[iSegment + 1];

        auto y_prev = geometry.bounds[iSegment];
        auto y_next = geometry.bounds[iSegment + 1];

        for(size_t j = 0; j < nLayers + 1; ++j) {
            points_r_prev[j] = center_prev + 0.5*w_prev*normal_w_prev + y_prev[j]*normal_h_prev;
            points_r_next[j] = center_next + 0.5*w_next*normal_w_next + y_next[j]*normal_h_next;
            points_l_prev[j] = center_prev - 0.5*w_prev*normal_w_prev + y_prev[j]*normal_h_prev;
            points_l_next[j] = center_next - 0.5*w_next*normal_w_next + y_next[j]*normal_h_next;
        }

        // Sides
        for(size_t iLayer = 0; iLayer < nLayers; ++iLayer) {
            // Only layers with height != 0
            if(y_prev[iLayer] != y_prev[iLayer+1] || y_next[iLayer] != y_next[iLayer+1]) {
                // Left
                addQuad(points_l_prev[iLayer], points_l_next[iLayer], points_l_next[iLayer+1], points_l_prev[iLayer+1], colors[iLayer]);

                // Right
                addQuad(points_r_prev[iLayer], points_r_prev[iLayer+1], points_r_next[iLayer+1], points_r_next[iLayer], colors[iLayer]);

                // Start
                if(iSegment == 0) {
                    addQuad(points_r_prev[iLayer], points_l_prev[iLayer], points_l_prev[iLayer+1], points_r_prev[iLayer+1], colors[iLayer]);
                }
                // End
                if(iSegment == nSegments - 1) {
                    addQuad(points_r_next[iLayer], points_r_next[iLayer+1], points_l_next[iLayer+1], points_l_next[iLayer], colors[iLayer]);
                }
            }
        }

        // Back
        for(size_t iLayer = 0; iLayer < nLayers; ++iLayer) {
            // Find first layer with height != 0
            if(y_prev[iLayer] != y_prev[iLayer+1] || y_next[iLayer] != y_next[iLayer+1]) {
                addQuad(points_l_prev[iLayer], points_r_prev[iLayer], points_r_next[iLayer], points_l_next[iLayer], colors[iLayer]);
                break;
            }
        }

        // Belly
        for(size_t iLayer = nLayers; iLayer > 0; --iLayer) {
            // Find first layer with height != 0
            if(y_prev[iLayer] != y_prev[iLayer-1] || y_next[iLayer] != y_next[iLayer-1]) {
                addQuad(points_l_prev[iLayer], points_l_next[iLayer], points_r_next[iLayer], points_r_prev[iLayer], colors[iLayer-1]);
                break;
            }
        }

        /*
        // Sides
        for(size_t jLayer = 0; jLayer < nLayers; ++jLayer) {
            // Only layers with height != 0
            if(y_prev[jLayer] != y_prev[jLayer + 1] || y_next[jLayer] != y_next[jLayer + 1]) {
                // Left
                addQuad(points_r_prev[jLayer], points_r_next[jLayer], points_r_next[jLayer + 1], points_r_prev[jLayer + 1], colors[jLayer]);

                // Right
                addQuad(points_l_next[jLayer], points_l_prev[jLayer], points_l_prev[jLayer + 1], points_l_next[jLayer + 1], colors[jLayer]);

                // Start
                if(iSegment == 0) {
                    addQuad(points_l_prev[jLayer], points_r_prev[jLayer], points_r_prev[jLayer + 1], points_l_prev[jLayer + 1], colors[jLayer]);
                }

                // End
                if(iSegment == nSegments - 1) {
                    addQuad(points_r_next[jLayer], points_l_next[jLayer], points_l_next[jLayer + 1], points_r_next[jLayer + 1], colors[jLayer]);
                }
            }
        }

        // Back

        for(size_t jLayer = nLayers - 1; jLayer >= 0; --jLayer) {
            // Iterate from the top down and only draw the back of the first layer with height != 0
            if(y_prev[jLayer] != y_prev[jLayer + 1] || y_next[jLayer] != y_next[jLayer + 1]) {
                addQuad(points_l_prev[jLayer + 1], points_r_prev[jLayer + 1], points_r_next[jLayer + 1], points_l_next[jLayer + 1], colors[jLayer]);
                break;
            }
        }

        for(size_t iLayer = 0; iLayer < nLayers; ++iLayer) {
            // Iterate from the bottom up and only draw the belly of the first layer with height != 0
            if(y_prev[iLayer] != y_prev[iLayer + 1] || y_next[iLayer] != y_next[iLayer + 1]) {
                addQuad(points_l_prev[iLayer], points_l_next[iLayer], points_r_next[iLayer], points_r_prev[iLayer], colors[iLayer]);
                break;
            }
        }
        */
    }
}


void LimbMesh::addQuad(const QVector3D& p0, const QVector3D& p1, const QVector3D& p2, const QVector3D& p3, const QColor& color) {
    // v5: previously pushed an x-negated copy into faces_left to fake the
    // opposite (symmetric) limb. With independent upper/lower meshes that
    // mirror copy lands on top of the other limb and made every bow look
    // symmetric in the GUI, so we now only emit the real faces.
    //
    // Lower-limb winding fix: in the lower-limb constructor we flip the
    // cross-section normal `normal_h` so the back/belly land on the same
    // side of the bow as the upper limb (otherwise the cross-section is
    // mirrored about its centerline and symmetric bows show a discontinuity
    // at the grip). That mirror also reverses the vertex winding of every
    // quad, so we reverse the order back here when the flip is in effect.
    if(reverse_winding) {
        faces_right.addQuad(p3, p2, p1, p0, color);
    } else {
        faces_right.addQuad(p0, p1, p2, p3, color);
    }
}
